use crate::libc;
use crate::{
    file::{File, Inode},
    fs::{fsinit, namei},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    memlayout::{kstack, TRAMPOLINE, TRAPFRAME},
    param::{NCPU, NOFILE, NPROC, ROOTDEV},
    printf::{panic, printf},
    riscv::{intr_get, intr_on, pagetable_t, r_tp, PGSIZE, PTE_R, PTE_W, PTE_X},
    spinlock::{pop_off, push_off, Spinlock},
    string::safestrcpy,
    trap::usertrapret,
    vm::{
        copyin, copyout, kvminithart, kvmmap, mappages, uvmalloc, uvmcopy, uvmcreate, uvmdealloc,
        uvmfree, uvminit, uvmunmap,
    },
};
use core::cmp::Ordering;
use core::ptr;

extern "C" {
    // swtch.S
    #[no_mangle]
    fn swtch(_: *mut Context, _: *mut Context);

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
}

#[derive(Copy, Clone)]
pub struct cpu {
    pub proc: *mut proc,
    pub scheduler: Context,
    pub noff: i32,
    pub intena: i32,
}

/// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
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
#[derive(Copy, Clone)]
pub struct proc {
    lock: Spinlock,
    pub state: procstate,
    parent: *mut proc,
    chan: *mut libc::c_void,
    pub killed: i32,
    xstate: i32,
    pub pid: i32,
    pub kstack: usize,
    pub sz: usize,
    pub pagetable: pagetable_t,
    pub tf: *mut trapframe,
    context: Context,
    pub ofile: [*mut File; NOFILE as usize],
    pub cwd: *mut Inode,
    pub name: [libc::c_char; 16],
}

/// Were interrupts enabled before push_off()?
/// per-process data for the trap handling code in trampoline.S.
/// sits in a page by itself just under the trampoline page in the
/// user page table. not specially mapped in the kernel page table.
/// the sscratch register points here.
/// uservec in trampoline.S saves user registers in the trapframe,
/// then initializes registers from the trapframe's
/// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
/// usertrapret() and userret in trampoline.S set up
/// the trapframe's kernel_*, restore user registers from the
/// trapframe, switch to the user page table, and enter user space.
/// the trapframe includes callee-saved user registers like s0-s11 because the
/// return-to-user path via usertrapret() doesn't return through
/// the entire kernel call stack.
#[derive(Copy, Clone)]
pub struct trapframe {
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub kernel_trap: usize,
    pub epc: usize,
    pub kernel_hartid: usize,
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
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
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
}

impl cpu {
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

impl proc {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
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
            ofile: [ptr::null_mut(); NOFILE as usize],
            cwd: ptr::null_mut(),
            name: [0; 16],
        }
    }
}

type procstate = u32;

pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;

static mut cpus: [cpu; NCPU as usize] = [cpu::zeroed(); NCPU as usize];

static mut proc: [proc; NPROC as usize] = [proc::zeroed(); NPROC as usize];

static mut initproc: *mut proc = ptr::null_mut();
static mut nextpid: i32 = 1;
static mut pid_lock: Spinlock = Spinlock::zeroed();

// trampoline.S
#[no_mangle]
pub unsafe fn procinit() {
    pid_lock.initlock(b"nextpid\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    let mut p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
        (*p).lock
            .initlock(b"proc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);

        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        let mut pa: *mut libc::c_char = kalloc() as *mut libc::c_char;
        if pa.is_null() {
            panic(b"kalloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        let mut va: usize =
            kstack(p.wrapping_offset_from(proc.as_mut_ptr()) as i64 as i32) as usize;
        kvmmap(va, pa as usize, PGSIZE as usize, (PTE_R | PTE_W) as i32);
        (*p).kstack = va;
        p = p.offset(1)
    }
    kvminithart();
}

/// Must be called with interrupts disabled,
/// to prevent race with process being moved
/// to a different CPU.
pub unsafe fn cpuid() -> i32 {
    let mut id: i32 = r_tp() as i32;
    id
}

/// Return this CPU's cpu struct.
/// Interrupts must be disabled.
pub unsafe fn mycpu() -> *mut cpu {
    let mut id: i32 = cpuid();
    let mut c: *mut cpu = &mut *cpus.as_mut_ptr().offset(id as isize) as *mut cpu;
    c
}

/// Return the current struct proc *, or zero if none.
pub unsafe fn myproc() -> *mut proc {
    push_off();
    let mut c: *mut cpu = mycpu();
    let mut p: *mut proc = (*c).proc;
    pop_off();
    p
}

unsafe fn allocpid() -> i32 {
    let mut pid: i32 = 0;
    pid_lock.acquire();
    pid = nextpid;
    nextpid += 1;
    pid_lock.release();
    pid
}

/// Look in the process table for an UNUSED proc.
/// If found, initialize state required to run in the kernel,
/// and return with p->lock held.
/// If there are no free procs, return 0.
unsafe fn allocproc() -> *mut proc {
    let mut current_block: usize;
    let mut p = proc.as_mut_ptr();
    loop {
        if p >= &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
            current_block = 7815301370352969686;
            break;
        }
        (*p).lock.acquire();
        if (*p).state as u32 == UNUSED as u32 {
            current_block = 17234009953499979309;
            break;
        }
        (*p).lock.release();
        p = p.offset(1)
    }
    match current_block {
        7815301370352969686 => ptr::null_mut(),
        _ => {
            (*p).pid = allocpid();

            // Allocate a trapframe page.
            (*p).tf = kalloc() as *mut trapframe;
            if (*p).tf.is_null() {
                (*p).lock.release();
                return ptr::null_mut();
            }

            // An empty user page table.
            (*p).pagetable = proc_pagetable(p);

            // Set up new context to start executing at forkret,
            // which returns to user space.
            ptr::write_bytes(&mut (*p).context as *mut Context, 0, 1);
            (*p).context.ra = forkret as usize;
            (*p).context.sp = (*p).kstack.wrapping_add(PGSIZE as usize);
            p
        }
    }
}

/// free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
unsafe fn freeproc(mut p: *mut proc) {
    if !(*p).tf.is_null() {
        kfree((*p).tf as *mut libc::c_void);
    }
    (*p).tf = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable((*p).pagetable, (*p).sz);
    }
    (*p).pagetable = 0 as pagetable_t;
    (*p).sz = 0;
    (*p).pid = 0;
    (*p).parent = ptr::null_mut();
    (*p).name[0] = 0 as libc::c_char;
    (*p).chan = ptr::null_mut();
    (*p).killed = 0;
    (*p).xstate = 0;
    (*p).state = UNUSED;
}

/// Create a page table for a given process,
/// with no user pages, but with trampoline pages.
pub unsafe fn proc_pagetable(mut p: *mut proc) -> pagetable_t {
    let mut pagetable: pagetable_t = ptr::null_mut();

    // An empty page table.
    pagetable = uvmcreate();

    // map the trampoline code (for system call return)
    // at the highest user virtual address.
    // only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    mappages(
        pagetable,
        TRAMPOLINE as usize,
        PGSIZE as usize,
        trampoline.as_mut_ptr() as usize,
        (PTE_R | PTE_X) as i32,
    );

    // map the trapframe just below TRAMPOLINE, for trampoline.S.
    mappages(
        pagetable,
        TRAPFRAME as usize,
        PGSIZE as usize,
        (*p).tf as usize,
        (PTE_R | PTE_W) as i32,
    );
    pagetable
}

/// Free a process's page table, and free the
/// physical memory it refers to.
pub unsafe fn proc_freepagetable(mut pagetable: pagetable_t, mut sz: usize) {
    uvmunmap(pagetable, TRAMPOLINE as usize, PGSIZE as usize, 0);
    uvmunmap(pagetable, TRAPFRAME as usize, PGSIZE as usize, 0);
    if sz > 0 {
        uvmfree(pagetable, sz);
    };
}

// a user program that calls exec("/init")
// od -t xC initcode
static mut initcode: [u8; 51] = [
    0x17, 0x5, 0, 0, 0x13, 0x5, 0x5, 0x2, 0x97, 0x5, 0, 0, 0x93, 0x85, 0x5, 0x2, 0x9d, 0x48, 0x73,
    0, 0, 0, 0x89, 0x48, 0x73, 0, 0, 0, 0xef, 0xf0, 0xbf, 0xff, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0, 0,
    0x1, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Set up first user process.
pub unsafe fn userinit() {
    let mut p: *mut proc = ptr::null_mut();
    p = allocproc();
    initproc = p;

    // allocate one user page and copy init's instructions
    // and data into it.
    uvminit(
        (*p).pagetable,
        initcode.as_mut_ptr(),
        ::core::mem::size_of::<[u8; 51]>() as u32,
    );
    (*p).sz = PGSIZE as usize;

    // prepare for the very first "return" from kernel to user.
    // user program counter
    (*(*p).tf).epc = 0;

    // user stack pointer
    (*(*p).tf).sp = PGSIZE as usize;
    safestrcpy(
        (*p).name.as_mut_ptr(),
        b"initcode\x00" as *const u8 as *const libc::c_char,
        ::core::mem::size_of::<[libc::c_char; 16]>() as i32,
    );
    (*p).cwd = namei(b"/\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    (*p).state = RUNNABLE;
    (*p).lock.release();
}

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub unsafe fn growproc(n: i32) -> i32 {
    let mut p: *mut proc = myproc();
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
    let mut pid: i32 = 0;
    let mut np: *mut proc = ptr::null_mut();
    let mut p: *mut proc = myproc();

    // Allocate process.
    np = allocproc();
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
        ::core::mem::size_of::<[libc::c_char; 16]>() as i32,
    );
    pid = (*np).pid;
    (*np).state = RUNNABLE;
    (*np).lock.release();
    pid
}

/// Pass p's abandoned children to init.
/// Caller must hold p->lock.
pub unsafe fn reparent(mut p: *mut proc) {
    let mut pp = proc.as_mut_ptr();
    while pp < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
        // this code uses pp->parent without holding pp->lock.
        // acquiring the lock first could cause a deadlock
        // if pp or a child of pp were also in exit()
        // and about to try to lock p.
        if (*pp).parent == p {
            // pp->parent can't change between the check and the acquire()
            // because only the parent changes it, and we're the parent.
            (*pp).lock.acquire();
            (*pp).parent = initproc;

            // we should wake up init here, but that would require
            // initproc->lock, which would be a deadlock, since we hold
            // the lock on one of init's children (pp). this is why
            // exit() always wakes init (before acquiring any locks).
            (*pp).lock.release();
        }
        pp = pp.offset(1)
    }
}

/// Exit the current process.  Does not return.
/// An exited process remains in the zombie state
/// until its parent calls wait().
pub unsafe fn exit(mut status: i32) {
    let mut p: *mut proc = myproc();
    if p == initproc {
        panic(b"init exiting\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }

    // Close all open files.
    for fd in 0..NOFILE {
        if !(*p).ofile[fd as usize].is_null() {
            let mut f: *mut File = (*p).ofile[fd as usize];
            (*f).close();
            (*p).ofile[fd as usize] = ptr::null_mut();
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
    (*initproc).lock.acquire();
    wakeup1(initproc);
    (*initproc).lock.release();

    // grab a copy of p->parent, to ensure that we unlock the same
    // parent we locked. in case our parent gives us away to init while
    // we're waiting for the parent lock. we may then race with an
    // exiting parent, but the result will be a harmless spurious wakeup
    // to a dead or wrong process; proc structs are never re-allocated
    // as anything else.
    (*p).lock.acquire();
    let mut original_parent: *mut proc = (*p).parent;
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
    panic(b"zombie exit\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// Wait for a child process to exit and return its pid.
/// Return -1 if this process has no children.
pub unsafe fn wait(mut addr: usize) -> i32 {
    let mut p: *mut proc = myproc();

    // hold p->lock for the whole time to avoid lost
    // wakeups from a child's exit().
    (*p).lock.acquire();
    loop {
        // Scan through table looking for exited children.
        let mut havekids: i32 = 0;
        let mut np = proc.as_mut_ptr();
        while np < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
            // this code uses np->parent without holding np->lock.
            // acquiring the lock first would cause a deadlock,
            // since np might be an ancestor, and we already hold p->lock.
            if (*np).parent == p {
                // np->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                (*np).lock.acquire();
                havekids = 1;
                if (*np).state as u32 == ZOMBIE as i32 as u32 {
                    // Found one.
                    let pid = (*np).pid;
                    if addr != 0
                        && copyout(
                            (*p).pagetable,
                            addr,
                            &mut (*np).xstate as *mut i32 as *mut libc::c_char,
                            ::core::mem::size_of::<i32>(),
                        ) < 0
                    {
                        (*np).lock.release();
                        (*p).lock.release();
                        return -1;
                    }
                    freeproc(np);
                    (*np).lock.release();
                    (*p).lock.release();
                    return pid;
                }
                (*np).lock.release();
            }
            np = np.offset(1)
        }

        // No point waiting if we don't have any children.
        if havekids == 0 || (*p).killed != 0 {
            (*p).lock.release();
            return -1;
        }

        // Wait for a child to exit.
        //DOC: wait-sleep
        sleep(p as *mut libc::c_void, &mut (*p).lock);
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
    let mut c: *mut cpu = mycpu();
    (*c).proc = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();
        let mut p = proc.as_mut_ptr();
        while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
            (*p).lock.acquire();
            if (*p).state as u32 == RUNNABLE as i32 as u32 {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                (*p).state = RUNNING;
                (*c).proc = p;
                swtch(&mut (*c).scheduler, &mut (*p).context);
                // Process is done running for now.
                // It should have changed its p->state before coming back.
                (*c).proc = ptr::null_mut()
            }
            (*p).lock.release();
            p = p.offset(1)
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
    let mut intena: i32 = 0;
    let mut p: *mut proc = myproc();
    if (*p).lock.holding() == 0 {
        panic(b"sched p->lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*mycpu()).noff != 1 {
        panic(b"sched locks\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*p).state as u32 == RUNNING as i32 as u32 {
        panic(b"sched running\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if intr_get() != 0 {
        panic(b"sched interruptible\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    intena = (*mycpu()).intena;
    swtch(
        &mut (*p).context,
        &mut (*(mycpu as unsafe fn() -> *mut cpu)()).scheduler,
    );
    (*mycpu()).intena = intena;
}

/// Give up the CPU for one scheduling round.
pub unsafe fn proc_yield() {
    let mut p: *mut proc = myproc();
    (*p).lock.acquire();
    (*p).state = RUNNABLE;
    sched();
    (*p).lock.release();
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
unsafe fn forkret() {
    static mut first: i32 = 1;

    // Still holding p->lock from scheduler.
    (*(myproc as unsafe fn() -> *mut proc)()).lock.release();
    if first != 0 {
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        first = 0;
        fsinit(ROOTDEV);
    }
    usertrapret();
}

/// Atomically release lock and sleep on chan.
/// reacquires lock when awakened.
pub unsafe fn sleep(mut chan: *mut libc::c_void, mut lk: *mut Spinlock) {
    let mut p: *mut proc = myproc();

    // Must acquire p->lock in order to
    // change p->state and then call sched.
    // Once we hold p->lock, we can be
    // guaranteed that we won't miss any wakeup
    // (wakeup locks p->lock),
    // so it's okay to release lk.

    //DOC: sleeplock0
    if lk != &mut (*p).lock as *mut Spinlock {
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

    // reacquire original lock.
    if lk != &mut (*p).lock as *mut Spinlock {
        (*p).lock.release();
        (*lk).acquire();
    };
}

/// Wake up all processes sleeping on chan.
/// Must be called without any p->lock.
pub unsafe fn wakeup(mut chan: *mut libc::c_void) {
    let mut p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
        (*p).lock.acquire();
        if (*p).state as u32 == SLEEPING as u32 && (*p).chan == chan {
            (*p).state = RUNNABLE
        }
        (*p).lock.release();
        p = p.offset(1)
    }
}

/// Wake up p if it is sleeping in wait(); used by exit().
/// Caller must hold p->lock.
unsafe fn wakeup1(mut p: *mut proc) {
    if (*p).lock.holding() == 0 {
        panic(b"wakeup1\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*p).chan == p as *mut libc::c_void && (*p).state as u32 == SLEEPING as u32 {
        (*p).state = RUNNABLE
    };
}

/// Kill the process with the given pid.
/// The victim won't exit until it tries to return
/// to user space (see usertrap() in trap.c).
pub unsafe fn kill(mut pid: i32) -> i32 {
    let mut p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
        (*p).lock.acquire();
        if (*p).pid == pid {
            (*p).killed = 1;
            if (*p).state as u32 == SLEEPING as u32 {
                // Wake process from sleep().
                (*p).state = RUNNABLE
            }
            (*p).lock.release();
            return 0;
        }
        (*p).lock.release();
        p = p.offset(1)
    }
    -1
}

/// Copy to either a user address, or kernel address,
/// depending on usr_dst.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyout(
    mut user_dst: i32,
    mut dst: usize,
    mut src: *mut libc::c_void,
    mut len: usize,
) -> i32 {
    let mut p: *mut proc = myproc();
    if user_dst != 0 {
        copyout((*p).pagetable, dst, src as *mut libc::c_char, len)
    } else {
        ptr::copy(src, dst as *mut libc::c_char as *mut libc::c_void, len);
        0
    }
}

/// Copy from either a user address, or kernel address,
/// depending on usr_src.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyin(
    mut dst: *mut libc::c_void,
    mut user_src: i32,
    mut src: usize,
    mut len: usize,
) -> i32 {
    let mut p: *mut proc = myproc();
    if user_src != 0 {
        copyin((*p).pagetable, dst as *mut libc::c_char, src, len)
    } else {
        ptr::copy(src as *mut libc::c_char as *const libc::c_void, dst, len);
        0
    }
}

/// Print a process listing to console.  For debugging.
/// Runs when user types ^P on console.
/// No lock to avoid wedging a stuck machine further.
pub unsafe fn procdump() {
    static mut states: [*mut libc::c_char; 5] = [
        b"unused\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"sleep \x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"runble\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"run   \x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"zombie\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    ];
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    let mut p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc {
        if (*p).state as u32 != UNUSED as i32 as u32 {
            let state = if ((*p).state as usize)
                < (::core::mem::size_of::<[*mut libc::c_char; 5]>() as usize)
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>())
                && !states[(*p).state as usize].is_null()
            {
                states[(*p).state as usize]
            } else {
                b"???\x00" as *const u8 as *const libc::c_char as *mut libc::c_char
            };
            printf(
                b"%d %s %s\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                (*p).pid,
                state,
                (*p).name.as_mut_ptr(),
            );
            printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        p = p.offset(1)
    }
}
