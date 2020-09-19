use crate::{
    file::{Inode, RcFile},
    fs::{fsinit, namei},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    memlayout::{kstack, TRAMPOLINE, TRAPFRAME},
    ok_or,
    param::{NCPU, NOFILE, NPROC, ROOTDEV},
    println,
    riscv::{intr_get, intr_on, r_tp, PGSIZE, PTE_R, PTE_W, PTE_X},
    spinlock::{pop_off, push_off, RawSpinlock, Spinlock},
    string::safestrcpy,
    trap::usertrapret,
    vm::{
        copyin, copyout, kvminithart, kvmmap, uvmalloc, uvmcopy, uvmdealloc, uvmfree, uvminit,
        uvmunmap,
    },
};
use crate::{libc, vm::mappages, vm::PageTable};
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

/// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,

    /// Callee-saved
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

/// Per-CPU-state.
#[derive(Copy, Clone)]
pub struct Cpu {
    /// The process running on this cpu, or null.
    pub proc: *mut Proc,

    /// swtch() here to enter scheduler().
    pub scheduler: Context,

    /// Depth of push_off() nesting.
    pub noff: i32,

    /// Were interrupts enabled before push_off()?
    pub interrupt_enabled: bool,
}

/// Per-process data for the trap handling code in trampoline.S.
/// Sits in a page by itself just under the trampoline page in the
/// user page table. Not specially mapped in the kernel page table.
/// The sscratch register points here.
/// uservec in trampoline.S saves user registers in the trapframe,
/// then initializes registers from the trapframe's
/// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
/// usertrapret() and userret in trampoline.S set up
/// the trapframe's kernel_*, restore user registers from the
/// trapframe, switch to the user page table, and enter user space.
/// The trapframe includes callee-saved user registers like s0-s11 because the
/// return-to-user path via usertrapret() doesn't return through
/// the entire kernel call stack.
#[derive(Copy, Clone)]
pub struct Trapframe {
    /// 0 - kernel page table (satp: Supervisor Address Translation and Protection)
    pub kernel_satp: usize,

    /// 8 - top of process's kernel stack
    pub kernel_sp: usize,

    /// 16 - usertrap()
    pub kernel_trap: usize,

    /// 24 - saved user program counter (ecp: Exception Program Counter)
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

#[derive(Copy, Clone, PartialEq)]
pub enum Procstate {
    ZOMBIE,
    RUNNING,
    RUNNABLE,
    SLEEPING,
    UNUSED,
}

pub struct WaitChannel {}

impl WaitChannel {
    pub const fn new() -> Self {
        Self {}
    }

    /// Atomically release lock and sleep on waitchannel.
    /// Reacquires lock when awakened.
    // TODO(@kimjungwow): lk is not SpinLockGuard yet because
    // 1. Some static mut variables are still not Spinlock<T> but RawSpinlock
    // 2. Sleeplock doesn't have Spinlock<T>
    pub unsafe fn sleep(&self, lk: *mut RawSpinlock) {
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
        (*p).waitchannel = self;
        (*p).state = Procstate::SLEEPING;
        sched();

        // Tidy up.
        (*p).waitchannel = ptr::null();

        // Reacquire original lock.
        if lk != &mut (*p).lock as *mut RawSpinlock {
            (*p).lock.release();
            (*lk).acquire();
        };
    }

    /// Wake up all processes sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup(&self) {
        unsafe { PROCSYS.wakeup_pool(self) }
    }

    /// Wake up p if it is sleeping in wait(); used by exit().
    /// Caller must hold p->lock.
    unsafe fn wakeup_proc(&self, p: *mut Proc) {
        if !(*p).lock.holding() {
            panic!("wakeup_proc");
        }
        if (*p).waitchannel == self as _ {
            (*p).wakeup()
        }
    }
}

/// Per-process state.
pub struct Proc {
    lock: RawSpinlock,

    /// p->lock must be held when using these:

    /// Process state.
    pub state: Procstate,

    /// Parent process.
    parent: *mut Proc,

    /// If non-zero, sleeping on waitchannel.
    waitchannel: *const WaitChannel,

    /// Waitchannel saying child proc is dead.
    child_waitchannel: WaitChannel,

    /// If true, the process have been killed.
    pub killed: bool,

    /// Exit status to be returned to parent's wait.
    xstate: i32,

    /// Process ID.
    pub pid: i32,

    /// These are private to the process, so p->lock need not be held.

    /// Bottom of kernel stack for this process.
    pub kstack: usize,

    /// Size of process memory (bytes).
    pub sz: usize,

    /// Page table.
    pub pagetable: PageTable,

    /// Data page for trampoline.S.
    pub tf: *mut Trapframe,

    /// swtch() here to run process.
    context: Context,

    /// Open files.
    pub open_files: [Option<RcFile>; NOFILE],

    /// Current directory.
    pub cwd: *mut Inode,

    /// Process name (debugging).
    pub name: [u8; 16],
}

impl Cpu {
    // TODO: transient measure.
    const fn zeroed() -> Self {
        Self {
            proc: ptr::null_mut(),
            scheduler: Context::zeroed(),
            noff: 0,
            interrupt_enabled: false,
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

impl Procstate {
    fn to_str(&self) -> &'static str {
        match self {
            Procstate::UNUSED => "unused",
            Procstate::SLEEPING => "sleep ",
            Procstate::RUNNABLE => "runble",
            Procstate::RUNNING => "run   ",
            Procstate::ZOMBIE => "zombie",
        }
    }
}

impl Proc {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            state: Procstate::UNUSED,
            parent: ptr::null_mut(),
            child_waitchannel: WaitChannel::new(),
            waitchannel: ptr::null(),
            killed: false,
            xstate: 0,
            pid: 0,
            kstack: 0,
            sz: 0,
            pagetable: unsafe { PageTable::raw() },
            tf: ptr::null_mut(),
            context: Context::zeroed(),
            open_files: [None; NOFILE],
            cwd: ptr::null_mut(),
            name: [0; 16],
        }
    }

    /// Allocate a page for the process's kernel stack.
    /// Map it high in memory, followed by an invalid
    /// guard page.
    unsafe fn palloc(&mut self, i: usize) {
        let pa = kalloc() as *mut u8;
        if pa.is_null() {
            panic!("kalloc");
        }
        let va: usize = kstack(i);
        kvmmap(va, pa as usize, PGSIZE, PTE_R | PTE_W);
        self.kstack = va;
    }

    /// Kill and wake the process up.
    fn kill(&mut self) {
        self.killed = true;
    }

    /// Wake process from sleep().
    fn wakeup(&mut self) {
        if self.state == Procstate::SLEEPING {
            self.state = Procstate::RUNNABLE
        }
    }

    /// Close all open files.
    unsafe fn close_files(&mut self) {
        for file in self.open_files.iter_mut() {
            *file = None;
        }
        begin_op();
        (*self.cwd).put();
        end_op();
        self.cwd = ptr::null_mut();
    }
}

static mut CPUS: [Cpu; NCPU] = [Cpu::zeroed(); NCPU];

/// Process system type containing & managing whole processes.
pub struct ProcessSystem {
    process_pool: [Proc; NPROC],
    initial_proc: *mut Proc,
}

impl ProcessSystem {
    const fn zeroed() -> Self {
        Self {
            process_pool: [Proc::zeroed(); NPROC],
            initial_proc: ptr::null_mut(),
        }
    }

    pub unsafe fn init(&mut self) {
        for (i, p) in self.process_pool.iter_mut().enumerate() {
            p.lock.initlock("proc");
            p.palloc(i);
        }
        kvminithart();
    }

    /// Look into process system for an UNUSED proc.
    /// If found, initialize state required to run in the kernel,
    /// and return with p->lock held.
    /// If there are no free procs, return 0.
    unsafe fn alloc(&mut self) -> Result<*mut Proc, ()> {
        for p in self.process_pool.iter_mut() {
            p.lock.acquire();
            if p.state == Procstate::UNUSED {
                p.pid = allocpid();

                // Allocate a trapframe page.
                p.tf = kalloc() as *mut Trapframe;
                if p.tf.is_null() {
                    p.lock.release();
                    return Err(());
                }

                // An empty user page table.
                p.pagetable = proc_pagetable(p);

                // Set up new context to start executing at forkret,
                // which returns to user space.
                ptr::write_bytes(&mut (*p).context as *mut Context, 0, 1);
                p.context.ra = forkret as usize;
                p.context.sp = p.kstack.wrapping_add(PGSIZE);
                return Ok(p);
            }
            p.lock.release();
        }

        Err(())
    }

    /// Pass p's abandoned children to init.
    /// Caller must hold p->lock.
    unsafe fn reparent(&mut self, p: *mut Proc) {
        for pp in self.process_pool.iter_mut() {
            // This code uses pp->parent without holding pp->lock.
            // Acquiring the lock first could cause a deadlock
            // if pp or a child of pp were also in exit()
            // and about to try to lock p.
            if pp.parent == p {
                // pp->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                pp.lock.acquire();
                pp.parent = self.initial_proc;

                // We should wake up init here, but that would require
                // PROCSYS.initial_proc->lock, which would be a deadlock, since we hold
                // the lock on one of init's children (pp). This is why
                // exit() always wakes init (before acquiring any locks).
                pp.lock.release();
            }
        }
    }

    /// Kill the process with the given pid.
    /// The victim won't exit until it tries to return
    /// to user space (see usertrap() in trap.c).
    pub unsafe fn kill(&mut self, pid: i32) -> i32 {
        for p in self.process_pool.iter_mut() {
            p.lock.acquire();
            if p.pid == pid {
                p.kill();
                p.wakeup();
                p.lock.release();
                return 0;
            }
            p.lock.release();
        }
        -1
    }

    /// Wake up all processes in the pool sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup_pool(&mut self, target: &WaitChannel) {
        for p in self.process_pool.iter_mut() {
            p.lock.acquire();
            if p.waitchannel == target as _ {
                p.wakeup()
            }
            p.lock.release();
        }
    }

    /// Set up first user process.
    pub unsafe fn user_proc_init(&mut self) {
        let mut p = self.alloc().expect("user_proc_init");

        self.initial_proc = p;

        // Allocate one user page and copy init's instructions
        // and data into it.
        uvminit(
            &mut (*p).pagetable,
            INITCODE.as_mut_ptr(),
            ::core::mem::size_of::<[u8; 51]>() as u32,
        );
        (*p).sz = PGSIZE;

        // Prepare for the very first "return" from kernel to user.

        // User program counter.
        (*(*p).tf).epc = 0;

        // User stack pointer.
        (*(*p).tf).sp = PGSIZE;
        safestrcpy(
            (*p).name.as_mut_ptr(),
            b"initcode\x00" as *const u8,
            ::core::mem::size_of::<[u8; 16]>() as i32,
        );
        (*p).cwd = namei(b"/\x00" as *const u8 as *mut u8);
        (*p).state = Procstate::RUNNABLE;
        (*p).lock.release();
    }

    /// Create a new process, copying the parent.
    /// Sets up child kernel stack to return as if from fork() system call.
    pub unsafe fn fork(&mut self) -> i32 {
        let p = myproc();

        // Allocate process.
        let mut np = ok_or!(self.alloc(), return -1);

        // Copy user memory from parent to child.
        if uvmcopy(&mut (*p).pagetable, &mut (*np).pagetable, (*p).sz) < 0 {
            freeproc(np);
            (*np).lock.release();
            return -1;
        }
        (*np).sz = (*p).sz;
        (*np).parent = p;

        // Copy saved user registers.
        *(*np).tf = *(*p).tf;

        // Cause fork to return 0 in the child.
        (*(*np).tf).a0 = 0;

        // Increment reference counts on open file descriptors.
        for i in 0..NOFILE {
            if let Some(file) = &(*p).open_files[i] {
                (*np).open_files[i] = Some(file.dup())
            }
        }
        (*np).cwd = (*(*p).cwd).idup();
        safestrcpy(
            (*np).name.as_mut_ptr(),
            (*p).name.as_mut_ptr(),
            ::core::mem::size_of::<[u8; 16]>() as i32,
        );
        let pid = (*np).pid;
        (*np).state = Procstate::RUNNABLE;
        (*np).lock.release();
        pid
    }

    /// Wait for a child process to exit and return its pid.
    /// Return -1 if this process has no children.
    pub unsafe fn wait(&mut self, addr: usize) -> i32 {
        let p: *mut Proc = myproc();

        // Hold p->lock for the whole time to avoid lost
        // Wakeups from a child's exit().
        (*p).lock.acquire();
        loop {
            // Scan through pool looking for exited children.
            let mut havekids = false;
            for np in self.process_pool.iter_mut() {
                // This code uses np->parent without holding np->lock.
                // Acquiring the lock first would cause a deadlock,
                // since np might be an ancestor, and we already hold p->lock.
                if np.parent == p {
                    // np->parent can't change between the check and the acquire()
                    // because only the parent changes it, and we're the parent.
                    np.lock.acquire();
                    havekids = true;
                    if np.state == Procstate::ZOMBIE {
                        let pid = np.pid;
                        if addr != 0
                            && copyout(
                                &mut (*p).pagetable,
                                addr,
                                &mut np.xstate as *mut i32 as *mut u8,
                                ::core::mem::size_of::<i32>(),
                            ).is_err()
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
            if !havekids || (*p).killed {
                (*p).lock.release();
                return -1;
            }

            // Wait for a child to exit.
            //DOC: wait-sleep
            (*p).child_waitchannel.sleep(&mut (*p).lock);
        }
    }

    /// Exit the current process.  Does not return.
    /// An exited process remains in the zombie state
    /// until its parent calls wait().
    pub unsafe fn exit_current(&mut self, status: i32) {
        let mut p = myproc();
        if p == self.initial_proc {
            panic!("init exiting");
        }

        (*p).close_files();

        // We might re-parent a child to init. We can't be precise about
        // waking up init, since we can't acquire its lock once we've
        // spinlock::acquired any other proc lock. so wake up init whether that's
        // necessary or not. init may miss this wakeup, but that seems
        // harmless.
        (*self.initial_proc).lock.acquire();
        (*self.initial_proc)
            .child_waitchannel
            .wakeup_proc(self.initial_proc);
        (*self.initial_proc).lock.release();

        // Grab a copy of p->parent, to ensure that we unlock the same
        // parent we locked. in case our parent gives us away to init while
        // we're waiting for the parent lock. We may then race with an
        // exiting parent, but the result will be a harmless spurious wakeup
        // to a dead or wrong process; proc structs are never re-allocated
        // as anything else.
        (*p).lock.acquire();
        let original_parent = (*p).parent;
        (*p).lock.release();

        // We need the parent's lock in order to wake it up from wait().
        // The parent-then-child rule says we have to lock it first.
        (*original_parent).lock.acquire();

        (*p).lock.acquire();

        // Give any children to init.
        self.reparent(p);

        // Parent might be sleeping in wait().
        (*original_parent)
            .child_waitchannel
            .wakeup_proc(original_parent);
        (*p).xstate = status;
        (*p).state = Procstate::ZOMBIE;
        (*original_parent).lock.release();

        // Jump into the scheduler, never to return.
        sched();
        panic!("zombie exit");
    }

    /// Print a process listing to console.  For debugging.
    /// Runs when user types ^P on console.
    /// No lock to avoid wedging a stuck machine further.
    pub unsafe fn dump(&mut self) {
        println!();
        for p in self.process_pool.iter_mut() {
            if p.state != Procstate::UNUSED {
                println!(
                    "{} {} {}",
                    p.pid,
                    Procstate::to_str(&p.state),
                    str::from_utf8(&p.name).unwrap_or("???")
                );
            }
        }
    }
}

/// Current process system.
pub static mut PROCSYS: ProcessSystem = ProcessSystem::zeroed();

/// Return this CPU's ID.
///
/// It is safe to call this function with interrupts enabled, but returned id may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn cpuid() -> usize {
    unsafe { r_tp() }
}

/// Return this CPU's cpu struct.
///
/// It is safe to call this function with interrupts enabled, but returned address may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn mycpu() -> *mut Cpu {
    let id: usize = cpuid();
    unsafe { &mut CPUS[id] as *mut Cpu }
}

/// Return the current struct Proc *, or zero if none.
pub unsafe fn myproc() -> *mut Proc {
    push_off();
    let c = mycpu();
    let p = (*c).proc;
    pop_off();
    p
}

fn allocpid() -> i32 {
    static NEXTPID: Spinlock<i32> = Spinlock::new("nextpid", 1);

    let mut pid = NEXTPID.lock();
    let ret = *pid;
    *pid += 1;
    ret
}

/// Free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
unsafe fn freeproc(mut p: *mut Proc) {
    if !(*p).tf.is_null() {
        kfree((*p).tf as *mut libc::CVoid);
    }
    (*p).tf = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable(&mut (*p).pagetable, (*p).sz);
    }
    (*p).pagetable = PageTable::raw();
    (*p).sz = 0;
    (*p).pid = 0;
    (*p).parent = ptr::null_mut();
    (*p).name[0] = 0;
    (*p).waitchannel = ptr::null();
    (*p).killed = false;
    (*p).xstate = 0;
    (*p).state = Procstate::UNUSED;
}

/// Create a page table for a given process,
/// with no user pages, but with trampoline pages.
pub unsafe fn proc_pagetable(p: *mut Proc) -> PageTable {
    // An empty page table.
    let mut pagetable = PageTable::raw();
    pagetable.init();
    // let mut pagetable = uvmcreate();

    // Map the trampoline code (for system call return)
    // at the highest user virtual address.
    // Only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    mappages(
        &mut pagetable,
        TRAMPOLINE,
        PGSIZE,
        trampoline.as_mut_ptr() as usize,
        PTE_R | PTE_X,
    );

    // Map the trapframe just below TRAMPOLINE, for trampoline.S.
    mappages(
        &mut pagetable,
        TRAPFRAME,
        PGSIZE,
        (*p).tf as usize,
        PTE_R | PTE_W,
    );

    pagetable
}

/// Free a process's page table, and free the
/// physical memory it refers to.
pub unsafe fn proc_freepagetable(pagetable: &mut PageTable, sz: usize) {
    uvmunmap(pagetable, TRAMPOLINE, PGSIZE, 0);
    uvmunmap(pagetable, TRAPFRAME, PGSIZE, 0);
    if sz > 0 {
        uvmfree(pagetable, sz);
    };
}

/// A user program that calls exec("/init").
/// od -t xC initcode
static mut INITCODE: [u8; 51] = [
    0x17, 0x5, 0, 0, 0x13, 0x5, 0x5, 0x2, 0x97, 0x5, 0, 0, 0x93, 0x85, 0x5, 0x2, 0x9d, 0x48, 0x73,
    0, 0, 0, 0x89, 0x48, 0x73, 0, 0, 0, 0xef, 0xf0, 0xbf, 0xff, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0, 0,
    0x1, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub unsafe fn resizeproc(n: i32) -> i32 {
    let mut p = myproc();
    let sz = (*p).sz;
    let sz = match n.cmp(&0) {
        Ordering::Equal => sz,
        Ordering::Greater => {
            let sz = uvmalloc(&mut (*p).pagetable, sz, sz.wrapping_add(n as usize));
            if sz.is_none() {
                return -1;
            }
            sz.unwrap()
        }
        Ordering::Less => uvmdealloc(&mut (*p).pagetable, sz, sz.wrapping_add(n as usize)),
    };
    (*p).sz = sz;
    0
}

/// Per-CPU process scheduler.
/// Each CPU calls scheduler() after setting itself up.
/// Scheduler never returns.  It loops, doing:
///  - choose a process to run.
///  - swtch to start running that process.
///  - eventually that process transfers control
///    via swtch back to the scheduler.
pub unsafe fn scheduler() -> ! {
    let mut c = mycpu();
    (*c).proc = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();

        for p in PROCSYS.process_pool.iter_mut() {
            p.lock.acquire();
            if p.state == Procstate::RUNNABLE {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                p.state = Procstate::RUNNING;
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
/// interrupt_enabled because interrupt_enabled is a property of this
/// kernel thread, not this CPU. It should
/// be proc->interrupt_enabled and proc->noff, but that would
/// break in the few places where a lock is held but
/// there's no process.
unsafe fn sched() {
    let p = myproc();
    if !(*p).lock.holding() {
        panic!("sched p->lock");
    }
    if (*mycpu()).noff != 1 {
        panic!("sched locks");
    }
    if (*p).state == Procstate::RUNNING {
        panic!("sched running");
    }
    if intr_get() {
        panic!("sched interruptible");
    }
    let interrupt_enabled = (*mycpu()).interrupt_enabled;
    swtch(
        &mut (*p).context,
        &mut (*(mycpu as unsafe fn() -> *mut Cpu)()).scheduler,
    );
    (*mycpu()).interrupt_enabled = interrupt_enabled;
}

/// Give up the CPU for one scheduling round.
pub unsafe fn proc_yield() {
    let mut p = myproc();
    (*p).lock.acquire();
    (*p).state = Procstate::RUNNABLE;
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

/// Copy to either a user address, or kernel address,
/// depending on usr_dst.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyout(user_dst: i32, dst: usize, src: *mut libc::CVoid, len: usize) -> i32 {
    let p = myproc();
    if user_dst != 0 {
        copyout(&mut (*p).pagetable, dst, src as *mut u8, len).map_or(-1, |_v| 0)
    } else {
        ptr::copy(src, dst as *mut u8 as *mut libc::CVoid, len);
        0
    }
}

/// Copy from either a user address, or kernel address,
/// depending on usr_src.
/// Returns 0 on success, -1 on error.
pub unsafe fn either_copyin(dst: *mut libc::CVoid, user_src: i32, src: usize, len: usize) -> i32 {
    let p = myproc();
    if user_src != 0 {
        copyin(&mut (*p).pagetable, dst as *mut u8, src, len).map_or(-1, |_v| 0)
    } else {
        ptr::copy(src as *mut u8 as *const libc::CVoid, dst, len);
        0
    }
}
