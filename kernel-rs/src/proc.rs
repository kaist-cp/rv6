use crate::libc;
use crate::{
    file::{fileclose, filedup, inode, File},
    fs::{fsinit, iput, namei},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    memlayout::{kstack, TRAMPOLINE, TRAPFRAME},
    param::{NCPU, NOFILE, NPROC, ROOTDEV},
    printf::{panic, printf},
    riscv::{intr_get, intr_on, pagetable_t, r_tp, PGSIZE, PTE_R, PTE_W, PTE_X},
    spinlock::{acquire, holding, initlock, pop_off, push_off, release, Spinlock},
    string::safestrcpy,
    trap::usertrapret,
    vm::{
        copyin, copyout, kvminithart, kvmmap, mappages, uvmalloc, uvmcopy, uvmcreate, uvmdealloc,
        uvmfree, uvminit, uvmunmap,
    },
};
use core::ptr;

extern "C" {
    // swtch.S
    #[no_mangle]
    fn swtch(_: *mut context, _: *mut context);
    // trampoline.S
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
}

#[derive(Copy, Clone)]
pub struct cpu {
    pub proc_0: *mut proc_0,
    pub scheduler: context,
    pub noff: i32,
    pub intena: i32,
}

/// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct context {
    pub ra: u64,
    pub sp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

/// Per-process state
#[derive(Copy, Clone)]
pub struct proc_0 {
    pub lock: Spinlock,
    pub state: procstate,
    pub parent: *mut proc_0,
    pub chan: *mut libc::c_void,
    pub killed: i32,
    pub xstate: i32,
    pub pid: i32,
    pub kstack: u64,
    pub sz: u64,
    pub pagetable: pagetable_t,
    pub tf: *mut trapframe,
    pub context: context,
    pub ofile: [*mut File; 16],
    pub cwd: *mut inode,
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
    pub kernel_satp: u64,
    pub kernel_sp: u64,
    pub kernel_trap: u64,
    pub epc: u64,
    pub kernel_hartid: u64,
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
}

pub type procstate = u32;

pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;

pub static mut cpus: [cpu; NCPU as usize] = [cpu {
    proc_0: ptr::null_mut(),
    scheduler: context {
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
    },
    noff: 0,
    intena: 0,
}; NCPU as usize];

#[export_name = "proc"]
pub static mut proc: [proc_0; 64] = [proc_0 {
    lock: Spinlock::zeroed(),
    state: UNUSED,
    parent: ptr::null_mut(),
    chan: 0 as *const libc::c_void as *mut libc::c_void,
    killed: 0,
    xstate: 0,
    pid: 0,
    kstack: 0,
    sz: 0,
    pagetable: 0 as *const u64 as *mut u64,
    tf: 0 as *const trapframe as *mut trapframe,
    context: context {
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
    },
    ofile: [0 as *const File as *mut File; 16],
    cwd: 0 as *const inode as *mut inode,
    name: [0; 16],
}; 64];

pub static mut initproc: *mut proc_0 = ptr::null_mut();
pub static mut nextpid: i32 = 1;
pub static mut pid_lock: Spinlock = Spinlock::zeroed();

// trampoline.S
#[no_mangle]
pub unsafe fn procinit() {
    let mut p: *mut proc_0 = ptr::null_mut();
    initlock(
        &mut pid_lock,
        b"nextpid\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        initlock(
            &mut (*p).lock,
            b"proc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        let mut pa: *mut libc::c_char = kalloc() as *mut libc::c_char;
        if pa.is_null() {
            panic(b"kalloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        let mut va: u64 = kstack(p.wrapping_offset_from(proc.as_mut_ptr()) as i64 as i32) as u64;
        kvmmap(va, pa as u64, PGSIZE as u64, (PTE_R | PTE_W) as i32);
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
pub unsafe fn myproc() -> *mut proc_0 {
    push_off();
    let mut c: *mut cpu = mycpu();
    let mut p: *mut proc_0 = (*c).proc_0;
    pop_off();
    p
}

pub unsafe fn allocpid() -> i32 {
    let mut pid: i32 = 0;
    acquire(&mut pid_lock);
    pid = nextpid;
    nextpid += 1;
    release(&mut pid_lock);
    pid
}

/// Look in the process table for an UNUSED proc.
/// If found, initialize state required to run in the kernel,
/// and return with p->lock held.
/// If there are no free procs, return 0.
unsafe fn allocproc() -> *mut proc_0 {
    let mut current_block: u64;
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    loop {
        if p >= &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            current_block = 7815301370352969686;
            break;
        }
        acquire(&mut (*p).lock);
        if (*p).state as u32 == UNUSED as u32 {
            current_block = 17234009953499979309;
            break;
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
    match current_block {
        7815301370352969686 => ptr::null_mut(),
        _ => {
            (*p).pid = allocpid();
            // Allocate a trapframe page.
            (*p).tf = kalloc() as *mut trapframe;
            if (*p).tf.is_null() {
                release(&mut (*p).lock);
                return ptr::null_mut();
            }
            // An empty user page table.
            (*p).pagetable = proc_pagetable(p);
            // Set up new context to start executing at forkret,
            // which returns to user space.
            ptr::write_bytes(&mut (*p).context as *mut context, 0, 1);
            (*p).context.ra = ::core::mem::transmute::<Option<unsafe fn() -> ()>, u64>(Some(
                forkret as unsafe fn() -> (),
            ));
            (*p).context.sp = (*p).kstack.wrapping_add(PGSIZE as u64);
            p
        }
    }
}

/// free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
unsafe fn freeproc(mut p: *mut proc_0) {
    if !(*p).tf.is_null() {
        kfree((*p).tf as *mut libc::c_void);
    }
    (*p).tf = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable((*p).pagetable, (*p).sz);
    }
    (*p).pagetable = 0 as pagetable_t;
    (*p).sz = 0 as i32 as u64;
    (*p).pid = 0 as i32;
    (*p).parent = ptr::null_mut();
    (*p).name[0 as i32 as usize] = 0 as i32 as libc::c_char;
    (*p).chan = ptr::null_mut();
    (*p).killed = 0 as i32;
    (*p).xstate = 0 as i32;
    (*p).state = UNUSED;
}

/// Create a page table for a given process,
/// with no user pages, but with trampoline pages.
pub unsafe fn proc_pagetable(mut p: *mut proc_0) -> pagetable_t {
    let mut pagetable: pagetable_t = ptr::null_mut();
    // An empty page table.
    pagetable = uvmcreate();
    // map the trampoline code (for system call return)
    // at the highest user virtual address.
    // only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    mappages(
        pagetable,
        TRAMPOLINE as u64,
        PGSIZE as u64,
        trampoline.as_mut_ptr() as u64,
        (PTE_R | PTE_X) as i32,
    );
    // map the trapframe just below TRAMPOLINE, for trampoline.S.
    mappages(
        pagetable,
        TRAPFRAME as u64,
        PGSIZE as u64,
        (*p).tf as u64,
        (PTE_R | PTE_W) as i32,
    );
    pagetable
}

/// Free a process's page table, and free the
/// physical memory it refers to.
pub unsafe fn proc_freepagetable(mut pagetable: pagetable_t, mut sz: u64) {
    uvmunmap(pagetable, TRAMPOLINE as u64, PGSIZE as u64, 0 as i32);
    uvmunmap(pagetable, TRAPFRAME as u64, PGSIZE as u64, 0 as i32);
    if sz > 0 as i32 as u64 {
        uvmfree(pagetable, sz);
    };
}

// a user program that calls exec("/init")
// od -t xC initcode
pub static mut initcode: [u8; 51] = [
    0x17 as u8, 0x5 as u8, 0 as u8, 0 as u8, 0x13 as u8, 0x5 as u8, 0x5 as u8, 0x2 as u8,
    0x97 as u8, 0x5 as u8, 0 as u8, 0 as u8, 0x93 as u8, 0x85 as u8, 0x5 as u8, 0x2 as u8,
    0x9d as u8, 0x48 as u8, 0x73 as u8, 0 as u8, 0 as u8, 0 as u8, 0x89 as u8, 0x48 as u8,
    0x73 as u8, 0 as u8, 0 as u8, 0 as u8, 0xef as u8, 0xf0 as u8, 0xbf as u8, 0xff as u8,
    0x2f as u8, 0x69 as u8, 0x6e as u8, 0x69 as u8, 0x74 as u8, 0 as u8, 0 as u8, 0x1 as u8,
    0x20 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8,
    0 as u8,
];

/// Set up first user process.
pub unsafe fn userinit() {
    let mut p: *mut proc_0 = ptr::null_mut();
    p = allocproc();
    initproc = p;
    // allocate one user page and copy init's instructions
    // and data into it.
    uvminit(
        (*p).pagetable,
        initcode.as_mut_ptr(),
        ::core::mem::size_of::<[u8; 51]>() as u64 as u32,
    );
    (*p).sz = PGSIZE as u64;
    // prepare for the very first "return" from kernel to user.
    (*(*p).tf).epc = 0 as i32 as u64; // user program counter
    (*(*p).tf).sp = PGSIZE as u64; // user stack pointer
    safestrcpy(
        (*p).name.as_mut_ptr(),
        b"initcode\x00" as *const u8 as *const libc::c_char,
        ::core::mem::size_of::<[libc::c_char; 16]>() as u64 as i32,
    );
    (*p).cwd = namei(b"/\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    (*p).state = RUNNABLE;
    release(&mut (*p).lock);
}

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub unsafe fn growproc(mut n: i32) -> i32 {
    let mut sz: u32 = 0;
    let mut p: *mut proc_0 = myproc();
    sz = (*p).sz as u32;
    if n > 0 as i32 {
        sz = uvmalloc((*p).pagetable, sz as u64, sz.wrapping_add(n as u32) as u64) as u32;
        if sz == 0 as i32 as u32 {
            return -(1 as i32);
        }
    } else if n < 0 as i32 {
        sz = uvmdealloc((*p).pagetable, sz as u64, sz.wrapping_add(n as u32) as u64) as u32
    }
    (*p).sz = sz as u64;
    0 as i32
}

/// Create a new process, copying the parent.
/// Sets up child kernel stack to return as if from fork() system call.
pub unsafe fn fork() -> i32 {
    let mut i: i32 = 0;
    let mut pid: i32 = 0;
    let mut np: *mut proc_0 = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    // Allocate process.
    np = allocproc();
    if np.is_null() {
        return -(1 as i32);
    }
    // Copy user memory from parent to child.
    if uvmcopy((*p).pagetable, (*np).pagetable, (*p).sz) < 0 as i32 {
        freeproc(np);
        release(&mut (*np).lock);
        return -(1 as i32);
    }
    (*np).sz = (*p).sz;
    (*np).parent = p;
    // copy saved user registers.
    *(*np).tf = *(*p).tf;
    // Cause fork to return 0 in the child.
    (*(*np).tf).a0 = 0 as i32 as u64;
    // increment reference counts on open file descriptors.
    i = 0 as i32;
    while i < NOFILE {
        if !(*p).ofile[i as usize].is_null() {
            (*np).ofile[i as usize] = filedup((*p).ofile[i as usize])
        }
        i += 1
    }
    (*np).cwd = (*(*p).cwd).idup();
    safestrcpy(
        (*np).name.as_mut_ptr(),
        (*p).name.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_char; 16]>() as u64 as i32,
    );
    pid = (*np).pid;
    (*np).state = RUNNABLE;
    release(&mut (*np).lock);
    pid
}

/// Pass p's abandoned children to init.
/// Caller must hold p->lock.
pub unsafe fn reparent(mut p: *mut proc_0) {
    let mut pp: *mut proc_0 = ptr::null_mut();
    pp = proc.as_mut_ptr();
    while pp < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        // this code uses pp->parent without holding pp->lock.
        // acquiring the lock first could cause a deadlock
        // if pp or a child of pp were also in exit()
        // and about to try to lock p.
        if (*pp).parent == p {
            // pp->parent can't change between the check and the acquire()
            // because only the parent changes it, and we're the parent.
            acquire(&mut (*pp).lock);
            (*pp).parent = initproc;
            // we should wake up init here, but that would require
            // initproc->lock, which would be a deadlock, since we hold
            // the lock on one of init's children (pp). this is why
            // exit() always wakes init (before acquiring any locks).
            release(&mut (*pp).lock);
        }
        pp = pp.offset(1)
    }
}

/// Exit the current process.  Does not return.
/// An exited process remains in the zombie state
/// until its parent calls wait().
pub unsafe fn exit(mut status: i32) {
    let mut p: *mut proc_0 = myproc();
    if p == initproc {
        panic(b"init exiting\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // Close all open files.
    let mut fd: i32 = 0 as i32;
    while fd < NOFILE {
        if !(*p).ofile[fd as usize].is_null() {
            let mut f: *mut File = (*p).ofile[fd as usize];
            fileclose(f);
            (*p).ofile[fd as usize] = ptr::null_mut();
        }
        fd += 1
    }
    begin_op();
    iput((*p).cwd);
    end_op();
    (*p).cwd = ptr::null_mut();
    // we might re-parent a child to init. we can't be precise about
    // waking up init, since we can't acquire its lock once we've
    // spinlock::acquired any other proc lock. so wake up init whether that's
    // necessary or not. init may miss this wakeup, but that seems
    // harmless.
    acquire(&mut (*initproc).lock);
    wakeup1(initproc);
    release(&mut (*initproc).lock);
    // grab a copy of p->parent, to ensure that we unlock the same
    // parent we locked. in case our parent gives us away to init while
    // we're waiting for the parent lock. we may then race with an
    // exiting parent, but the result will be a harmless spurious wakeup
    // to a dead or wrong process; proc structs are never re-allocated
    // as anything else.
    acquire(&mut (*p).lock);
    let mut original_parent: *mut proc_0 = (*p).parent;
    release(&mut (*p).lock);
    // we need the parent's lock in order to wake it up from wait().
    // the parent-then-child rule says we have to lock it first.
    acquire(&mut (*original_parent).lock);
    acquire(&mut (*p).lock);
    // Give any children to init.
    reparent(p);
    // Parent might be sleeping in wait().
    wakeup1(original_parent);
    (*p).xstate = status;
    (*p).state = ZOMBIE;
    release(&mut (*original_parent).lock);
    // Jump into the scheduler, never to return.
    sched();
    panic(b"zombie exit\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// Wait for a child process to exit and return its pid.
/// Return -1 if this process has no children.
pub unsafe fn wait(mut addr: u64) -> i32 {
    let mut np: *mut proc_0 = ptr::null_mut();
    let mut havekids: i32 = 0;
    let mut pid: i32 = 0;
    let mut p: *mut proc_0 = myproc();
    // hold p->lock for the whole time to avoid lost
    // wakeups from a child's exit().
    acquire(&mut (*p).lock);
    loop {
        // Scan through table looking for exited children.
        havekids = 0 as i32;
        np = proc.as_mut_ptr();
        while np < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            //DOC: wait-sleep
            // this code uses np->parent without holding np->lock.
            // acquiring the lock first would cause a deadlock,
            // since np might be an ancestor, and we already hold p->lock.
            if (*np).parent == p {
                // np->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                acquire(&mut (*np).lock);
                havekids = 1 as i32;
                if (*np).state as u32 == ZOMBIE as i32 as u32 {
                    // Found one.
                    pid = (*np).pid;
                    if addr != 0 as i32 as u64
                        && copyout(
                            (*p).pagetable,
                            addr,
                            &mut (*np).xstate as *mut i32 as *mut libc::c_char,
                            ::core::mem::size_of::<i32>() as u64,
                        ) < 0 as i32
                    {
                        release(&mut (*np).lock);
                        release(&mut (*p).lock);
                        return -(1 as i32);
                    }
                    freeproc(np);
                    release(&mut (*np).lock);
                    release(&mut (*p).lock);
                    return pid;
                }
                release(&mut (*np).lock);
            }
            np = np.offset(1)
        }
        if havekids == 0 || (*p).killed != 0 {
            release(&mut (*p).lock);
            return -(1 as i32);
        }
        sleep(p as *mut libc::c_void, &mut (*p).lock);
    }
}

/// No point waiting if we don't have any children.
/// Wait for a child to exit.
/// Per-CPU process scheduler.
/// Each CPU calls scheduler() after setting itself up.
/// Scheduler never returns.  It loops, doing:
///  - choose a process to run.
///  - swtch to start running that process.
///  - eventually that process transfers control
///    via swtch back to the scheduler.
pub unsafe fn scheduler() -> ! {
    let mut p: *mut proc_0 = ptr::null_mut();
    let mut c: *mut cpu = mycpu();
    (*c).proc_0 = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();
        p = proc.as_mut_ptr();
        while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            acquire(&mut (*p).lock);
            if (*p).state as u32 == RUNNABLE as i32 as u32 {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                (*p).state = RUNNING;
                (*c).proc_0 = p;
                swtch(&mut (*c).scheduler, &mut (*p).context);
                // Process is done running for now.
                // It should have changed its p->state before coming back.
                (*c).proc_0 = ptr::null_mut()
            }
            release(&mut (*p).lock);
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
pub unsafe fn sched() {
    let mut intena: i32 = 0;
    let mut p: *mut proc_0 = myproc();
    if holding(&mut (*p).lock) == 0 {
        panic(b"sched p->lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*mycpu()).noff != 1 as i32 {
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
#[export_name = "yield"]
pub unsafe fn yield_0() {
    let mut p: *mut proc_0 = myproc();
    acquire(&mut (*p).lock);
    (*p).state = RUNNABLE;
    sched();
    release(&mut (*p).lock);
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
pub unsafe fn forkret() {
    static mut first: i32 = 1 as i32;
    // Still holding p->lock from scheduler.
    release(&mut (*(myproc as unsafe fn() -> *mut proc_0)()).lock);
    if first != 0 {
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        first = 0 as i32;
        fsinit(ROOTDEV);
    }
    usertrapret();
}

/// Atomically release lock and sleep on chan.
/// reacquires lock when awakened.
pub unsafe fn sleep(mut chan: *mut libc::c_void, mut lk: *mut Spinlock) {
    let mut p: *mut proc_0 = myproc();
    // Must acquire p->lock in order to
    // change p->state and then call sched.
    // Once we hold p->lock, we can be
    // guaranteed that we won't miss any wakeup
    // (wakeup locks p->lock),
    // so it's okay to release lk.
    if lk != &mut (*p).lock as *mut Spinlock {
        //DOC: sleeplock0
        acquire(&mut (*p).lock); //DOC: sleeplock1
        release(lk);
    }
    // Go to sleep.
    (*p).chan = chan;
    (*p).state = SLEEPING;
    sched();
    // Tidy up.
    (*p).chan = ptr::null_mut();
    // reacquire original lock.
    if lk != &mut (*p).lock as *mut Spinlock {
        release(&mut (*p).lock);
        acquire(lk);
    };
}

/// Wake up all processes sleeping on chan.
/// Must be called without any p->lock.
pub unsafe fn wakeup(mut chan: *mut libc::c_void) {
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        acquire(&mut (*p).lock);
        if (*p).state as u32 == SLEEPING as u32 && (*p).chan == chan {
            (*p).state = RUNNABLE
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
}

/// Wake up p if it is sleeping in wait(); used by exit().
/// Caller must hold p->lock.
unsafe fn wakeup1(mut p: *mut proc_0) {
    if holding(&mut (*p).lock) == 0 {
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
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        acquire(&mut (*p).lock);
        if (*p).pid == pid {
            (*p).killed = 1 as i32;
            if (*p).state as u32 == SLEEPING as u32 {
                // Wake process from sleep().
                (*p).state = RUNNABLE
            }
            release(&mut (*p).lock);
            return 0 as i32;
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
    -1
}

/// Copy to either a user address, or kernel address,
/// depending on usr_dst.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyout(
    mut user_dst: i32,
    mut dst: u64,
    mut src: *mut libc::c_void,
    mut len: u64,
) -> i32 {
    let mut p: *mut proc_0 = myproc();
    if user_dst != 0 {
        copyout((*p).pagetable, dst, src as *mut libc::c_char, len)
    } else {
        ptr::copy(
            src,
            dst as *mut libc::c_char as *mut libc::c_void,
            len as usize,
        );
        0
    }
}

/// Copy from either a user address, or kernel address,
/// depending on usr_src.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyin(
    mut dst: *mut libc::c_void,
    mut user_src: i32,
    mut src: u64,
    mut len: u64,
) -> i32 {
    let mut p: *mut proc_0 = myproc();
    if user_src != 0 {
        copyin((*p).pagetable, dst as *mut libc::c_char, src, len)
    } else {
        ptr::copy(
            src as *mut libc::c_char as *const libc::c_void,
            dst,
            len as usize,
        );
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
    let mut p: *mut proc_0 = ptr::null_mut();
    let mut state: *mut libc::c_char = ptr::null_mut();
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        if (*p).state as u32 != UNUSED as i32 as u32 {
            if (*p).state as u32 >= 0 as i32 as u32
                && ((*p).state as u64)
                    < (::core::mem::size_of::<[*mut libc::c_char; 5]>() as u64)
                        .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as u64)
                && !states[(*p).state as usize].is_null()
            {
                state = states[(*p).state as usize]
            } else {
                state = b"???\x00" as *const u8 as *const libc::c_char as *mut libc::c_char
            }
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
