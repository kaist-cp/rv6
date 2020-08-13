use crate::libc;
use crate::{
    file::{File, Inode},
    fs::{fsinit, namei},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    memlayout::{kstack, TRAMPOLINE, TRAPFRAME},
    param::{NCPU, NOFILE, NPROC, ROOTDEV},
    println,
    riscv::{intr_get, intr_on, r_tp, PagetableT, PGSIZE, PTE_R, PTE_W, PTE_X},
    spinlock::{pop_off, push_off, RawSpinlock},
    string::safestrcpy,
    trap::usertrapret,
    vm::{
        copyin, copyout, kvminithart, kvmmap, mappages, uvmalloc, uvmcopy, uvmcreate, uvmdealloc,
        uvmfree, uvminit, uvmunmap,
    },
};
use core::cmp::Ordering;
use core::ptr;
use core::str;

extern "C" {
    // swtch.S
    #[no_mangle]
    fn swtch(_: *mut Context, _: *mut Context);

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [u8; 0];
}

/// per-CPU-state
#[derive(Copy, Clone)]
pub struct Cpu {
    /// The process running on this cpu, or null
    pub proc: *mut Proc,

    /// swtch() here to enter scheduler()
    pub scheduler: Context,

    /// Depth of push_off() nesting
    pub noff: i32,

    /// Were interrupts enabled before push_off()?
    pub intena: i32,
}

/// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,

    /// callee-saved
    pub s0: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
}

/// Per-process state
pub struct Proc {
    lock: RawSpinlock,

    /// p->lock must be held when using these:
    /// Process state
    pub state: Procstate,

    /// Parent process
    parent: *mut Proc,

    /// If non-zero, sleeping on chan
    chan: *mut libc::CVoid,

    /// If non-zero, have been killed
    pub killed: i32,

    /// Exit status to be returned to parent's wait
    xstate: i32,

    /// Process ID
    pub pid: i32,

    /// these are private to the process, so p->lock need not be held.
    /// Bottom of kernel stack for this process
    pub kstack: usize,

    /// Size of process memory (bytes)
    pub sz: usize,

    /// Page table
    pub pagetable: PagetableT,

    /// data page for trampoline.S
    pub tf: *mut Trapframe,

    /// swtch() here to run process
    context: Context,

    /// Open files
    pub ofile: [*mut File; NOFILE],

    /// Current directory
    pub cwd: *mut Inode,

    /// Process name (debugging)
    pub name: [u8; 16],
}

/// per-process data for the trap handling code in trampoline.S.
/// sits in a page by itself just under the trampoline page in the
/// user page table. not specially mapped in the kernel page table.
/// the sscratch register points here.
/// uservec in trampoline.S saves user registers in the Trapframe,
/// then initializes registers from the trapframe's
/// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
/// usertrapret() and userret in trampoline.S set up
/// the trapframe's kernel_*, restore user registers from the
/// trapframe, switch to the user page table, and enter user space.
/// the trapframe includes callee-saved user registers like s0-s11 because the
/// return-to-user path via usertrapret() doesn't return through
/// the entire kernel call stack.
#[derive(Copy, Clone)]
pub struct Trapframe {
    /// 0 - kernel page table
    pub kernel_satp: usize,

    /// 8 - top of process's kernel stack
    pub kernel_sp: usize,

    /// 16 - usertrap()
    pub kernel_trap: usize,

    /// 24 - saved user program counter
    pub epc: usize,

    /// 32 - saved kernel tp
    pub kernel_hartid: usize,

    /// 40
    pub ra: usize,

    /// 48
    pub sp: usize,

    /// 56
    pub gp: usize,

    /// 64
    pub tp: usize,

    /// 72
    pub t0: usize,

    /// 80
    pub t1: usize,

    /// 88
    pub t2: usize,

    /// 96
    pub s0: usize,

    /// 104
    pub s1: usize,

    /// 112
    pub a0: usize,

    /// 120
    pub a1: usize,

    /// 128
    pub a2: usize,

    /// 136
    pub a3: usize,

    /// 144
    pub a4: usize,

    /// 152
    pub a5: usize,

    /// 160
    pub a6: usize,

    /// 168
    pub a7: usize,

    /// 176
    pub s2: usize,

    /// 184
    pub s3: usize,

    /// 192
    pub s4: usize,

    /// 200
    pub s5: usize,

    /// 208
    pub s6: usize,

    /// 216
    pub s7: usize,

    /// 224
    pub s8: usize,

    /// 232
    pub s9: usize,

    /// 240
    pub s10: usize,

    /// 248
    pub s11: usize,

    /// 256
    pub t3: usize,

    /// 264
    pub t4: usize,

    /// 272
    pub t5: usize,

    /// 280
    pub t6: usize,
}

impl Cpu {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            proc: ptr::null_mut(),
            scheduler: Context::zeroed(),
            noff: 0,
            intena: 0,
        }
    }
}

impl Context {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s0: 0,
            s1: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
        }
    }
}

impl Proc {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            state: UNUSED,
            parent: ptr::null_mut(),
            chan: ptr::null_mut(),
            killed: 0,
            xstate: 0,
            pid: 0,
            kstack: 0,
            sz: 0,
            pagetable: ptr::null_mut(),
            tf: ptr::null_mut(),
            context: Context::zeroed(),
            ofile: [ptr::null_mut(); NOFILE],
            cwd: ptr::null_mut(),
            name: [0; 16],
        }
    }
}

type Procstate = u32;

pub const ZOMBIE: Procstate = 4;
pub const RUNNING: Procstate = 3;
pub const RUNNABLE: Procstate = 2;
pub const SLEEPING: Procstate = 1;
pub const UNUSED: Procstate = 0;

static mut CPUS: [Cpu; NCPU] = [Cpu::zeroed(); NCPU];

static mut PROC: [Proc; NPROC] = [Proc::zeroed(); NPROC];

static mut INITPROC: *mut Proc = ptr::null_mut();

static mut NEXTPID: i32 = 1;

static mut PID_LOCK: RawSpinlock = RawSpinlock::zeroed();

#[no_mangle]
pub unsafe fn procinit() {
    PID_LOCK.initlock("nextpid");
    for (i, p) in PROC.iter_mut().enumerate() {
        p.lock.initlock("proc");

        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        let pa: *mut u8 = kalloc() as *mut u8;
        if pa.is_null() {
            panic!("kalloc");
        }
        let va: usize = kstack(i as _);
        kvmmap(va, pa as usize, PGSIZE, PTE_R | PTE_W);
        p.kstack = va;
    }
    kvminithart();
}

/// Must be called with interrupts disabled,
/// to prevent race with process being moved
/// to a different CPU.
pub unsafe fn cpuid() -> i32 {
    let id: i32 = r_tp() as i32;
    id
}

/// Return this CPU's cpu struct.
/// Interrupts must be disabled.
pub unsafe fn mycpu() -> *mut Cpu {
    let id: i32 = cpuid();
    let c: *mut Cpu = &mut *CPUS.as_mut_ptr().offset(id as isize) as *mut Cpu;
    c
}

/// Return the current struct Proc *, or zero if none.
pub unsafe fn myproc() -> *mut Proc {
    push_off();
    let c: *mut Cpu = mycpu();
    let p: *mut Proc = (*c).proc;
    pop_off();
    p
}

unsafe fn allocpid() -> i32 {
    PID_LOCK.acquire();
    let pid: i32 = NEXTPID;
    NEXTPID += 1;
    PID_LOCK.release();
    pid
}

/// Look in the process table for an UNUSED proc.
/// If found, initialize state required to run in the kernel,
/// and return with p->lock held.
/// If there are no free procs, return 0.
unsafe fn allocproc() -> *mut Proc {
    for p in &mut PROC[..] {
        p.lock.acquire();
        if p.state as u32 == UNUSED as u32 {
            p.pid = allocpid();

            // Allocate a trapframe page.
            p.tf = kalloc() as *mut Trapframe;
            if p.tf.is_null() {
                p.lock.release();
                return ptr::null_mut();
            }

            // An empty user page table.
            p.pagetable = proc_pagetable(p);

            // Set up new context to start executing at forkret,
            // which returns to user space.
            ptr::write_bytes(&mut (*p).context as *mut Context, 0, 1);
            p.context.ra = forkret as usize;
            p.context.sp = p.kstack.wrapping_add(PGSIZE);
            return p;
        }
        p.lock.release();
    }

    ptr::null_mut()
}

/// free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
unsafe fn freeproc(mut p: *mut Proc) {
    if !(*p).tf.is_null() {
        kfree((*p).tf as *mut libc::CVoid);
    }
    (*p).tf = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable((*p).pagetable, (*p).sz);
    }
    (*p).pagetable = 0 as PagetableT;
    (*p).sz = 0;
    (*p).pid = 0;
    (*p).parent = ptr::null_mut();
    (*p).name[0] = 0;
    (*p).chan = ptr::null_mut();
    (*p).killed = 0;
    (*p).xstate = 0;
    (*p).state = UNUSED;
}

/// Create a page table for a given process,
/// with no user pages, but with trampoline pages.
pub unsafe fn proc_pagetable(p: *mut Proc) -> PagetableT {
    // An empty page table.
    let pagetable: PagetableT = uvmcreate();

    // map the trampoline code (for system call return)
    // at the highest user virtual address.
    // only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    mappages(
        pagetable,
        TRAMPOLINE,
        PGSIZE,
        trampoline.as_mut_ptr() as usize,
        PTE_R | PTE_X,
    );

    // map the trapframe just below TRAMPOLINE, for trampoline.S.
    mappages(
        pagetable,
        TRAPFRAME,
        PGSIZE,
        (*p).tf as usize,
        PTE_R | PTE_W,
    );
    pagetable
}

/// Free a process's page table, and free the
/// physical memory it refers to.
pub unsafe fn proc_freepagetable(pagetable: PagetableT, sz: usize) {
    uvmunmap(pagetable, TRAMPOLINE, PGSIZE, 0);
    uvmunmap(pagetable, TRAPFRAME, PGSIZE, 0);
    if sz > 0 {
        uvmfree(pagetable, sz);
    };
}

/// a user program that calls exec("/init")
/// od -t xC initcode
static mut INITCODE: [u8; 51] = [
    0x17, 0x5, 0, 0, 0x13, 0x5, 0x5, 0x2, 0x97, 0x5, 0, 0, 0x93, 0x85, 0x5, 0x2, 0x9d, 0x48, 0x73,
    0, 0, 0, 0x89, 0x48, 0x73, 0, 0, 0, 0xef, 0xf0, 0xbf, 0xff, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0, 0,
    0x1, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Set up first user process.
pub unsafe fn userinit() {
    let mut p: *mut Proc = allocproc();
    INITPROC = p;

    // allocate one user page and copy init's instructions
    // and data into it.
    uvminit(
        (*p).pagetable,
        INITCODE.as_mut_ptr(),
        ::core::mem::size_of::<[u8; 51]>() as u32,
    );
    (*p).sz = PGSIZE;

    // prepare for the very first "return" from kernel to user.
    // user program counter
    (*(*p).tf).epc = 0;

    // user stack pointer
    (*(*p).tf).sp = PGSIZE;
    safestrcpy(
        (*p).name.as_mut_ptr(),
        b"initcode\x00" as *const u8,
        ::core::mem::size_of::<[u8; 16]>() as i32,
    );
    (*p).cwd = namei(b"/\x00" as *const u8 as *mut u8);
    (*p).state = RUNNABLE;
    (*p).lock.release();
}

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub unsafe fn growproc(n: i32) -> i32 {
    let mut p: *mut Proc = myproc();
    let sz = (*p).sz as u32;
    let sz = match n.cmp(&0) {
        Ordering::Equal => sz,
        Ordering::Greater => {
            let sz = uvmalloc(
                (*p).pagetable,
                sz as usize,
                sz.wrapping_add(n as u32) as usize,
            ) as u32;
            if sz == 0 {
                return -1;
            }
            sz
        }
        Ordering::Less => uvmdealloc(
            (*p).pagetable,
            sz as usize,
            sz.wrapping_add(n as u32) as usize,
        ) as u32,
    };
    (*p).sz = sz as usize;
    0
}

/// Create a new process, copying the parent.
/// Sets up child kernel stack to return as if from fork() system call.
pub unsafe fn fork() -> i32 {
    let p: *mut Proc = myproc();

    // Allocate process.
    let mut np: *mut Proc = allocproc();
    if np.is_null() {
        return -1;
    }

    // Copy user memory from parent to child.
    if uvmcopy((*p).pagetable, (*np).pagetable, (*p).sz) < 0 {
        freeproc(np);
        (*np).lock.release();
        return -1;
    }
    (*np).sz = (*p).sz;
    (*np).parent = p;

    // copy saved user registers.
    *(*np).tf = *(*p).tf;

    // Cause fork to return 0 in the child.
    (*(*np).tf).a0 = 0;

    // increment reference counts on open file descriptors.
    for i in 0..NOFILE {
        if !(*p).ofile[i as usize].is_null() {
            (*np).ofile[i as usize] = (*(*p).ofile[i as usize]).dup()
        }
    }
    (*np).cwd = (*(*p).cwd).idup();
    safestrcpy(
        (*np).name.as_mut_ptr(),
        (*p).name.as_mut_ptr(),
        ::core::mem::size_of::<[u8; 16]>() as i32,
    );
    let pid: i32 = (*np).pid;
    (*np).state = RUNNABLE;
    (*np).lock.release();
    pid
}

/// Pass p's abandoned children to init.
/// Caller must hold p->lock.
pub unsafe fn reparent(p: *mut Proc) {
    for pp in &mut PROC[..] {
        // this code uses pp->parent without holding pp->lock.
        // acquiring the lock first could cause a deadlock
        // if pp or a child of pp were also in exit()
        // and about to try to lock p.
        if pp.parent == p {
            // pp->parent can't change between the check and the acquire()
            // because only the parent changes it, and we're the parent.
            pp.lock.acquire();
            pp.parent = INITPROC;

            // we should wake up init here, but that would require
            // initproc->lock, which would be a deadlock, since we hold
            // the lock on one of init's children (pp). this is why
            // exit() always wakes init (before acquiring any locks).
            pp.lock.release();
        }
    }
}

/// Exit the current process.  Does not return.
/// An exited process remains in the zombie state
/// until its parent calls wait().
pub unsafe fn exit(status: i32) {
    let mut p: *mut Proc = myproc();
    if p == INITPROC {
        panic!("init exiting");
    }

    // Close all open files.
    for fd in 0..NOFILE {
        if !(*p).ofile[fd].is_null() {
            let f: *mut File = (*p).ofile[fd];
            (*f).close();
            (*p).ofile[fd] = ptr::null_mut();
        }
    }
    begin_op();
    (*(*p).cwd).put();
    end_op();
    (*p).cwd = ptr::null_mut();

    // we might re-parent a child to init. we can't be precise about
    // waking up init, since we can't acquire its lock once we've
    // spinlock::acquired any other proc lock. so wake up init whether that's
    // necessary or not. init may miss this wakeup, but that seems
    // harmless.
    (*INITPROC).lock.acquire();
    wakeup1(INITPROC);
    (*INITPROC).lock.release();

    // grab a copy of p->parent, to ensure that we unlock the same
    // parent we locked. in case our parent gives us away to init while
    // we're waiting for the parent lock. we may then race with an
    // exiting parent, but the result will be a harmless spurious wakeup
    // to a dead or wrong process; proc structs are never re-allocated
    // as anything else.
    (*p).lock.acquire();
    let original_parent: *mut Proc = (*p).parent;
    (*p).lock.release();

    // we need the parent's lock in order to wake it up from wait().
    // the parent-then-child rule says we have to lock it first.
    (*original_parent).lock.acquire();

    (*p).lock.acquire();

    // Give any children to init.
    reparent(p);

    // Parent might be sleeping in wait().
    wakeup1(original_parent);
    (*p).xstate = status;
    (*p).state = ZOMBIE;
    (*original_parent).lock.release();

    // Jump into the scheduler, never to return.
    sched();
    panic!("zombie exit");
}

/// Wait for a child process to exit and return its pid.
/// Return -1 if this process has no children.
pub unsafe fn wait(addr: usize) -> i32 {
    let p: *mut Proc = myproc();

    // hold p->lock for the whole time to avoid lost
    // wakeups from a child's exit().
    (*p).lock.acquire();
    loop {
        // Scan through table looking for exited children.
        let mut havekids: i32 = 0;
        for np in &mut PROC[..] {
            // this code uses np->parent without holding np->lock.
            // acquiring the lock first would cause a deadlock,
            // since np might be an ancestor, and we already hold p->lock.
            if np.parent == p {
                // np->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                np.lock.acquire();
                havekids = 1;
                if np.state as u32 == ZOMBIE as i32 as u32 {
                    // Found one.
                    let pid = np.pid;
                    if addr != 0
                        && copyout(
                            (*p).pagetable,
                            addr,
                            &mut np.xstate as *mut i32 as *mut u8,
                            ::core::mem::size_of::<i32>(),
                        ) < 0
                    {
                        np.lock.release();
                        (*p).lock.release();
                        return -1;
                    }
                    freeproc(np);
                    np.lock.release();
                    (*p).lock.release();
                    return pid;
                }
                np.lock.release();
            }
        }

        // No point waiting if we don't have any children.
        if havekids == 0 || (*p).killed != 0 {
            (*p).lock.release();
            return -1;
        }

        // Wait for a child to exit.
        //DOC: wait-sleep
        sleep(p as *mut libc::CVoid, &mut (*p).lock);
    }
}

/// Per-CPU process scheduler.
/// Each CPU calls scheduler() after setting itself up.
/// Scheduler never returns.  It loops, doing:
///  - choose a process to run.
///  - swtch to start running that process.
///  - eventually that process transfers control
///    via swtch back to the scheduler.
pub unsafe fn scheduler() -> ! {
    let mut c: *mut Cpu = mycpu();
    (*c).proc = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();

        for p in &mut PROC[..] {
            p.lock.acquire();
            if p.state as u32 == RUNNABLE as i32 as u32 {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                p.state = RUNNING;
                (*c).proc = p;
                swtch(&mut (*c).scheduler, &mut p.context);

                // Process is done running for now.
                // It should have changed its p->state before coming back.
                (*c).proc = ptr::null_mut()
            }
            p.lock.release();
        }
    }
}

/// Switch to scheduler.  Must hold only p->lock
/// and have changed proc->state. Saves and restores
/// intena because intena is a property of this
/// kernel thread, not this CPU. It should
/// be proc->intena and proc->noff, but that would
/// break in the few places where a lock is held but
/// there's no process.
unsafe fn sched() {
    let p: *mut Proc = myproc();
    if (*p).lock.holding() == 0 {
        panic!("sched p->lock");
    }
    if (*mycpu()).noff != 1 {
        panic!("sched locks");
    }
    if (*p).state as u32 == RUNNING as i32 as u32 {
        panic!("sched running");
    }
    if intr_get() != 0 {
        panic!("sched interruptible");
    }
    let intena: i32 = (*mycpu()).intena;
    swtch(
        &mut (*p).context,
        &mut (*(mycpu as unsafe fn() -> *mut Cpu)()).scheduler,
    );
    (*mycpu()).intena = intena;
}

/// Give up the CPU for one scheduling round.
pub unsafe fn proc_yield() {
    let mut p: *mut Proc = myproc();
    (*p).lock.acquire();
    (*p).state = RUNNABLE;
    sched();
    (*p).lock.release();
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
unsafe fn forkret() {
    static mut FIRST: i32 = 1;

    // Still holding p->lock from scheduler.
    (*(myproc as unsafe fn() -> *mut Proc)()).lock.release();
    if FIRST != 0 {
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        FIRST = 0;
        fsinit(ROOTDEV);
    }
    usertrapret();
}

/// Atomically release lock and sleep on chan.
/// reacquires lock when awakened.
pub unsafe fn sleep(chan: *mut libc::CVoid, lk: *mut RawSpinlock) {
    let mut p: *mut Proc = myproc();

    // Must acquire p->lock in order to
    // change p->state and then call sched.
    // Once we hold p->lock, we can be
    // guaranteed that we won't miss any wakeup
    // (wakeup locks p->lock),
    // so it's okay to release lk.

    //DOC: sleeplock0
    if lk != &mut (*p).lock as *mut RawSpinlock {
        //DOC: sleeplock1
        (*p).lock.acquire();
        (*lk).release();
    }

    // Go to sleep.
    (*p).chan = chan;
    (*p).state = SLEEPING;
    sched();

    // Tidy up.
    (*p).chan = ptr::null_mut();

    // Reacquire original lock.
    if lk != &mut (*p).lock as *mut RawSpinlock {
        (*p).lock.release();
        (*lk).acquire();
    };
}

/// Wake up all processes sleeping on chan.
/// Must be called without any p->lock.
pub unsafe fn wakeup(chan: *mut libc::CVoid) {
    for p in &mut PROC[..] {
        p.lock.acquire();
        if p.state as u32 == SLEEPING as u32 && p.chan == chan {
            p.state = RUNNABLE
        }
        p.lock.release();
    }
}

/// Wake up p if it is sleeping in wait(); used by exit().
/// Caller must hold p->lock.
unsafe fn wakeup1(mut p: *mut Proc) {
    if (*p).lock.holding() == 0 {
        panic!("wakeup1");
    }
    if (*p).chan == p as *mut libc::CVoid && (*p).state as u32 == SLEEPING as u32 {
        (*p).state = RUNNABLE
    };
}

/// Kill the process with the given pid.
/// The victim won't exit until it tries to return
/// to user space (see usertrap() in trap.c).
pub unsafe fn kill(pid: i32) -> i32 {
    for p in &mut PROC[..] {
        p.lock.acquire();
        if p.pid == pid {
            p.killed = 1;
            if p.state as u32 == SLEEPING as u32 {
                // Wake process from sleep().
                p.state = RUNNABLE
            }
            p.lock.release();
            return 0;
        }
        p.lock.release();
    }
    -1
}

/// Copy to either a user address, or kernel address,
/// depending on usr_dst.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyout(user_dst: i32, dst: usize, src: *mut libc::CVoid, len: usize) -> i32 {
    let p: *mut Proc = myproc();
    if user_dst != 0 {
        copyout((*p).pagetable, dst, src as *mut u8, len)
    } else {
        ptr::copy(src, dst as *mut u8 as *mut libc::CVoid, len);
        0
    }
}

/// Copy from either a user address, or kernel address,
/// depending on usr_src.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyin(dst: *mut libc::CVoid, user_src: i32, src: usize, len: usize) -> i32 {
    let p: *mut Proc = myproc();
    if user_src != 0 {
        copyin((*p).pagetable, dst as *mut u8, src, len)
    } else {
        ptr::copy(src as *mut u8 as *const libc::CVoid, dst, len);
        0
    }
}

/// Print a process listing to console.  For debugging.
/// Runs when user types ^P on console.
/// No lock to avoid wedging a stuck machine further.
pub unsafe fn procdump() {
    static mut STATES: [&str; 5] = ["unused", "sleep ", "runble", "run   ", "zombie"];
    println!();
    for p in &mut PROC[..] {
        if p.state as u32 != UNUSED as i32 as u32 {
            let state = STATES.get(p.state as usize).unwrap_or(&"???");
            println!(
                "{} {} {}",
                p.pid,
                state,
                str::from_utf8(&p.name).unwrap_or("???")
            );
        }
    }
}
