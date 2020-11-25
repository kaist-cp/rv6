#![allow(clippy::unit_arg)]

use core::{
    cell::UnsafeCell,
    cmp, mem,
    ops::{Deref, DerefMut},
    ptr, slice, str,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use crate::{
    file::RcFile,
    fs::{Path, RcInode},
    kernel::kernel,
    memlayout::{kstack, TRAMPOLINE, TRAPFRAME},
    ok_or,
    page::Page,
    param::{MAXPROCNAME, NOFILE, NPROC, ROOTDEV},
    println,
    riscv::{intr_get, intr_on, r_tp, PGSIZE, PTE_R, PTE_W, PTE_X},
    sleepablelock::SleepablelockGuard,
    some_or,
    spinlock::{pop_off, push_off, RawSpinlock, Spinlock, SpinlockGuard},
    string::safestrcpy,
    trap::usertrapret,
    vm::{KVAddr, PAddr, PageTable, UVAddr, VAddr},
};

extern "C" {
    // swtch.S
    fn swtch(_: *mut Context, _: *mut Context);

    // trampoline.S
    static mut trampoline: [u8; 0];
}

/// Saved registers for kernel context switches.
#[derive(Copy, Clone, Default)]
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
    pub context: Context,

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

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Procstate {
    ZOMBIE,
    RUNNING,
    RUNNABLE,
    SLEEPING,
    UNUSED,
}

pub struct WaitChannel {
    /// Required to make this type non-zero-sized. If it were zero-sized, multiple wait channels may
    /// have the same address, spuriously waking up more threads.
    _padding: u8,
}

impl WaitChannel {
    pub const fn new() -> Self {
        Self { _padding: 0 }
    }

    /// Atomically release lock and sleep on waitchannel.
    /// Reacquires lock when awakened.
    pub unsafe fn sleep<T>(&self, guard: &mut SpinlockGuard<'_, T>) {
        self.sleep_raw(guard.raw() as *mut RawSpinlock);
    }

    /// Atomically release lock and sleep on waitchannel.
    /// Reacquires lock when awakened.
    pub unsafe fn sleep_sleepable<T>(&self, guard: &mut SleepablelockGuard<'_, T>) {
        self.sleep_raw(guard.raw() as *mut RawSpinlock);
    }

    /// Atomically release lock and sleep on waitchannel.
    /// Reacquires lock when awakened.
    // TODO(@kimjungwow): lk is not SpinlockGuard yet because
    // 1. Some static mut variables are still not Spinlock<T> but RawSpinlock
    // 2. Sleeplock doesn't have Spinlock<T>
    pub unsafe fn sleep_raw(&self, lk: *mut RawSpinlock) {
        let p: *mut Proc = myproc();

        // Must acquire p->lock in order to
        // change p->state and then call sched.
        // Once we hold p->lock, we can be
        // guaranteed that we won't miss any wakeup
        // (wakeup locks p->lock),
        // so it's okay to release lk.

        //DOC: sleeplock0
        if lk != (*p).info.raw() as *mut RawSpinlock {
            //DOC: sleeplock1
            mem::forget((*p).info.lock());
            (*lk).release();
        }

        // Go to sleep.
        let mut guard = ProcGuard::from_raw(p);
        guard.deref_mut_info().waitchannel = self;
        guard.deref_mut_info().state = Procstate::SLEEPING;
        guard.sched();

        // Tidy up.
        guard.deref_mut_info().waitchannel = ptr::null();
        mem::forget(guard);

        // Reacquire original lock.
        if lk != (*p).info.raw() as *mut RawSpinlock {
            (*p).info.unlock();
            (*lk).acquire();
        };
    }

    /// Wake up all processes sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup(&self) {
        kernel().procs.wakeup_pool(self)
    }
}

/// Proc::info's spinlock must be held when using these.
struct ProcInfo {
    /// Process state.
    state: Procstate,

    /// Parent process.
    parent: *mut Proc,

    /// If non-zero, sleeping on waitchannel.
    waitchannel: *const WaitChannel,

    /// Waitchannel saying child proc is dead.
    child_waitchannel: WaitChannel,

    /// Exit status to be returned to parent's wait.
    xstate: i32,

    /// Process ID.
    pid: i32,
}

/// Proc::data are private to the process, so lock need not be held.
pub struct ProcData {
    /// Virtual address of kernel stack.
    pub kstack: usize,

    /// Size of process memory (bytes).
    pub sz: usize,

    /// User Page table.
    pub pagetable: PageTable<UVAddr>,

    /// Data page for trampoline.S.
    pub trapframe: *mut Trapframe,

    /// swtch() here to run process.
    context: Context,

    /// Open files.
    pub open_files: [Option<RcFile>; NOFILE],

    /// Current directory.
    pub cwd: Option<RcInode>,
}

/// Per-process state.
pub struct Proc {
    info: Spinlock<ProcInfo>,

    pub data: UnsafeCell<ProcData>,

    /// If true, the process have been killed.
    killed: AtomicBool,

    /// Process name (debugging).
    pub name: [u8; MAXPROCNAME],
}

/// Assumption: `ptr` is `myproc()`, and ptr->info's spinlock is held.
struct ProcGuard {
    ptr: *const Proc,
}

impl ProcGuard {
    fn deref_info(&self) -> &ProcInfo {
        unsafe { (*self.ptr).info.get_mut_unchecked() }
    }

    fn deref_mut_info(&mut self) -> &mut ProcInfo {
        unsafe { (*self.ptr).info.get_mut_unchecked() }
    }

    unsafe fn from_raw(ptr: *const Proc) -> Self {
        Self { ptr }
    }

    fn raw(&self) -> *const Proc {
        self.ptr
    }

    /// Wake up p if it is sleeping in wait(); used by exit().
    /// Caller must hold p->lock.
    unsafe fn wakeup_proc(&mut self) {
        let info = self.deref_mut_info();
        if &info.child_waitchannel as *const _ == info.waitchannel {
            self.wakeup();
        }
    }

    /// Switch to scheduler.  Must hold only p->lock
    /// and have changed proc->state. Saves and restores
    /// interrupt_enabled because interrupt_enabled is a property of this
    /// kernel thread, not this CPU. It should
    /// be proc->interrupt_enabled and proc->noff, but that would
    /// break in the few places where a lock is held but
    /// there's no process.
    unsafe fn sched(&mut self) {
        assert_eq!((*kernel().mycpu()).noff, 1, "sched locks");
        assert_ne!(self.deref_info().state, Procstate::RUNNING, "sched running");
        assert!(!intr_get(), "sched interruptible");

        let interrupt_enabled = (*kernel().mycpu()).interrupt_enabled;
        swtch(
            &mut (*self.data.get()).context,
            &mut (*kernel().mycpu()).context,
        );
        (*kernel().mycpu()).interrupt_enabled = interrupt_enabled;
    }
}

impl Drop for ProcGuard {
    fn drop(&mut self) {
        unsafe {
            let proc = &*self.ptr;
            proc.info.unlock();
        }
    }
}

impl Deref for ProcGuard {
    type Target = Proc;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for ProcGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.ptr as *mut _) }
    }
}

impl Cpu {
    pub const fn new() -> Self {
        Self {
            proc: ptr::null_mut(),
            context: Context::new(),
            noff: 0,
            interrupt_enabled: false,
        }
    }
}

impl Context {
    const fn new() -> Self {
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

impl ProcData {
    const fn new() -> Self {
        Self {
            kstack: 0,
            sz: 0,
            pagetable: PageTable::zero(),
            trapframe: ptr::null_mut(),
            context: Context::new(),
            open_files: [None; NOFILE],
            cwd: None,
        }
    }

    /// Allocate a page for the process's kernel stack.
    /// Map it high in memory, followed by an invalid
    /// guard page.
    unsafe fn palloc(&mut self, page_table: &mut PageTable<KVAddr>, i: usize) {
        let page = kernel().alloc().expect("kalloc");
        let pa = page.into_usize();

        let va: usize = kstack(i);
        page_table.kvmmap(
            KVAddr::new(va),
            PAddr::new(pa as usize),
            PGSIZE,
            PTE_R | PTE_W,
        );
        self.kstack = va;
    }

    /// Close all open files.
    unsafe fn close_files(&mut self) {
        for file in &mut self.open_files {
            *file = None;
        }
        let _tx = kernel().fs().begin_transaction();
        self.cwd = None;
    }
}

/// TODO(@efenniht): pid, state, wakeup should be methods of ProcGuard.
impl Proc {
    const fn zero() -> Self {
        Self {
            info: Spinlock::new(
                "proc",
                ProcInfo {
                    state: Procstate::UNUSED,
                    parent: ptr::null_mut(),
                    child_waitchannel: WaitChannel::new(),
                    waitchannel: ptr::null(),
                    xstate: 0,
                    pid: 0,
                },
            ),
            data: UnsafeCell::new(ProcData::new()),
            killed: AtomicBool::new(false),
            name: [0; MAXPROCNAME],
        }
    }

    fn lock(&self) -> ProcGuard {
        mem::forget(self.info.lock());
        ProcGuard { ptr: self }
    }

    pub unsafe fn pid(&self) -> i32 {
        self.info.get_mut_unchecked().pid
    }

    pub unsafe fn state(&self) -> Procstate {
        self.info.get_mut_unchecked().state
    }

    /// Kill and wake the process up.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Release);
    }

    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }

    /// Wake process from sleep().
    fn wakeup(&mut self) {
        if self.info.get_mut().state == Procstate::SLEEPING {
            self.info.get_mut().state = Procstate::RUNNABLE
        }
    }
}

/// Process system type containing & managing whole processes.
pub struct ProcessSystem {
    nextpid: AtomicI32,
    process_pool: [Proc; NPROC],
    initial_proc: *mut Proc,
}

const fn proc_entry(_: usize) -> Proc {
    Proc::zero()
}

impl ProcessSystem {
    pub const fn zero() -> Self {
        Self {
            nextpid: AtomicI32::new(1),
            process_pool: array_const_fn_init![proc_entry; 64],
            initial_proc: ptr::null_mut(),
        }
    }

    fn allocpid(&self) -> i32 {
        self.nextpid.fetch_add(1, Ordering::Relaxed)
    }

    /// Look into process system for an UNUSED proc.
    /// If found, initialize state required to run in the kernel,
    /// and return with p->lock held.
    /// If there are no free procs, return 0.
    unsafe fn alloc(&self) -> Result<ProcGuard, ()> {
        for p in &self.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().state == Procstate::UNUSED {
                let data = &mut *guard.data.get();
                guard.deref_mut_info().pid = self.allocpid();

                // Allocate a trapframe page.
                let page = some_or!(kernel().alloc(), return Err(()));
                data.trapframe = page.into_usize() as *mut Trapframe;

                // An empty user page table.
                data.pagetable = proc_pagetable(p as *const _ as *mut _);

                // Set up new context to start executing at forkret,
                // which returns to user space.
                data.context = Default::default();
                data.context.ra = forkret as usize;
                data.context.sp = data.kstack.wrapping_add(PGSIZE);
                return Ok(guard);
            }
        }

        Err(())
    }

    /// Pass p's abandoned children to init.
    /// Caller must hold p->lock.
    unsafe fn reparent(&self, p: &mut ProcGuard) {
        for pp in &self.process_pool {
            // This code uses pp->parent without holding pp->lock.
            // Acquiring the lock first could cause a deadlock
            // if pp or a child of pp were also in exit()
            // and about to try to lock p.
            if pp.info.get_mut_unchecked().parent == p.raw() as *mut _ {
                // pp->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                let mut guard = pp.lock();
                guard.deref_mut_info().parent = self.initial_proc;

                // We should wake up init here, but that would require
                // kernel().procs.initial_proc->lock, which would be a deadlock, since we hold
                // the lock on one of init's children (pp). This is why
                // exit() always wakes init (before acquiring any locks).
            }
        }
    }

    /// Kill the process with the given pid.
    /// The victim won't exit until it tries to return
    /// to user space (see usertrap() in trap.c).
    pub fn kill(&self, pid: i32) -> i32 {
        for p in &self.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().pid == pid {
                p.kill();
                guard.wakeup();
                return 0;
            }
        }
        -1
    }

    /// Wake up all processes in the pool sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup_pool(&self, target: &WaitChannel) {
        for p in &self.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().waitchannel == target as _ {
                guard.wakeup()
            }
        }
    }

    /// Set up first user process.
    pub unsafe fn user_proc_init(&mut self) {
        let mut guard = self.alloc().expect("user_proc_init");

        self.initial_proc = guard.raw() as *mut _;

        let data = &mut *guard.data.get();
        // Allocate one user page and copy init's instructions
        // and data into it.
        data.pagetable.uvminit(&INITCODE);
        data.sz = PGSIZE;

        // Prepare for the very first "return" from kernel to user.

        // User program counter.
        (*data.trapframe).epc = 0;

        // User stack pointer.
        (*data.trapframe).sp = PGSIZE;
        safestrcpy(
            (*guard).name.as_mut_ptr(),
            b"initcode\x00" as *const u8,
            mem::size_of::<[u8; MAXPROCNAME]>() as i32,
        );
        data.cwd = Some(Path::root());
        guard.deref_mut_info().state = Procstate::RUNNABLE;
    }

    /// Create a new process, copying the parent.
    /// Sets up child kernel stack to return as if from fork() system call.
    pub unsafe fn fork(&self) -> i32 {
        let p = myproc();

        // Allocate process.
        let mut np = ok_or!(self.alloc(), return -1);

        let pdata = &mut *(*p).data.get();
        let mut npdata = &mut *np.data.get();
        // Copy user memory from parent to child.
        if pdata
            .pagetable
            .uvmcopy(&mut npdata.pagetable, pdata.sz)
            .is_err()
        {
            freeproc(np);
            return -1;
        }
        npdata.sz = pdata.sz;
        np.deref_mut_info().parent = p;

        // Copy saved user registers.
        *npdata.trapframe = *pdata.trapframe;

        // Cause fork to return 0 in the child.
        (*npdata.trapframe).a0 = 0;

        // Increment reference counts on open file descriptors.
        for i in 0..NOFILE {
            if let Some(file) = &pdata.open_files[i] {
                npdata.open_files[i] = Some(file.clone())
            }
        }
        npdata.cwd = Some(pdata.cwd.clone().unwrap());
        safestrcpy(
            (*np).name.as_mut_ptr(),
            (*p).name.as_mut_ptr(),
            mem::size_of::<[u8; MAXPROCNAME]>() as i32,
        );
        let pid = np.deref_mut_info().pid;
        np.deref_mut_info().state = Procstate::RUNNABLE;
        pid
    }

    /// Wait for a child process to exit and return its pid.
    /// Return -1 if this process has no children.
    pub unsafe fn wait(&self, addr: UVAddr) -> i32 {
        let p: *mut Proc = myproc();
        let data = &mut *(*p).data.get();

        // Hold p->lock for the whole time to avoid lost
        // Wakeups from a child's exit().
        let mut guard = (*p).lock();
        loop {
            // Scan through pool looking for exited children.
            let mut havekids = false;
            for np in &self.process_pool {
                // This code uses np->parent without holding np->lock.
                // Acquiring the lock first would cause a deadlock,
                // since np might be an ancestor, and we already hold p->lock.
                if np.info.get_mut_unchecked().parent == p {
                    // np->parent can't change between the check and the acquire()
                    // because only the parent changes it, and we're the parent.
                    let mut np = np.lock();
                    havekids = true;
                    if np.deref_info().state == Procstate::ZOMBIE {
                        let pid = np.deref_info().pid;
                        if !addr.is_null()
                            && data
                                .pagetable
                                .copyout(
                                    addr,
                                    slice::from_raw_parts_mut(
                                        &mut np.deref_mut_info().xstate as *mut i32 as *mut u8,
                                        mem::size_of::<i32>(),
                                    ),
                                )
                                .is_err()
                        {
                            return -1;
                        }
                        freeproc(np);
                        return pid;
                    }
                }
            }

            // No point waiting if we don't have any children.
            if !havekids || (*p).killed() {
                return -1;
            }

            // Wait for a child to exit.
            //DOC: wait-sleep
            guard
                .deref_mut_info()
                .child_waitchannel
                .sleep_raw((*p).info.raw() as *mut _);
        }
    }

    /// Exit the current process.  Does not return.
    /// An exited process remains in the zombie state
    /// until its parent calls wait().
    pub unsafe fn exit_current(&self, status: i32) -> ! {
        let p = myproc();
        let data = &mut *(*p).data.get();
        assert_ne!(p, self.initial_proc, "init exiting");

        data.close_files();

        // We might re-parent a child to init. We can't be precise about
        // waking up init, since we can't acquire its lock once we've
        // spinlock::acquired any other proc lock. so wake up init whether that's
        // necessary or not. init may miss this wakeup, but that seems
        // harmless.
        let mut initial_proc = (*self.initial_proc).lock();
        initial_proc.wakeup_proc();
        drop(initial_proc);

        // Grab a copy of p->parent, to ensure that we unlock the same
        // parent we locked. in case our parent gives us away to init while
        // we're waiting for the parent lock. We may then race with an
        // exiting parent, but the result will be a harmless spurious wakeup
        // to a dead or wrong process; proc structs are never re-allocated
        // as anything else.
        let guard = (*p).lock();
        let original_parent = guard.deref_info().parent;
        drop(guard);

        // We need the parent's lock in order to wake it up from wait().
        // The parent-then-child rule says we have to lock it first.
        let mut original_parent = (*original_parent).lock();

        let mut guard = (*p).lock();

        // Give any children to init.
        self.reparent(&mut guard);

        // Parent might be sleeping in wait().
        original_parent.wakeup_proc();
        guard.deref_mut_info().xstate = status;
        guard.deref_mut_info().state = Procstate::ZOMBIE;
        drop(original_parent);

        // Jump into the scheduler, never to return.
        guard.sched();

        unreachable!("zombie exit")
    }

    /// Print a process listing to console.  For debugging.
    /// Runs when user types ^P on console.
    /// No lock to avoid wedging a stuck machine further.
    pub fn dump(&self) {
        println!();
        for p in &self.process_pool {
            // For null character recognization.
            // str::from_utf8 cannot recognize interior null characters.
            let mut name = [0; MAXPROCNAME];
            let mut count = 0;
            while p.name[count] != 0 && count < MAXPROCNAME {
                name[count] = p.name[count];
                count += 1;
            }
            unsafe {
                let info = p.info.get_mut_unchecked();
                if info.state != Procstate::UNUSED {
                    println!(
                        "{} {} {}",
                        info.pid,
                        Procstate::to_str(&info.state),
                        str::from_utf8(&name).unwrap_or("???")
                    );
                }
            }
        }
    }
}

/// Initialize the proc table at boot time.
pub unsafe fn procinit(procs: &mut ProcessSystem, page_table: &mut PageTable<KVAddr>) {
    for (i, p) in procs.process_pool.iter_mut().enumerate() {
        (*p.data.get()).palloc(page_table, i);
    }
}

/// Return this CPU's ID.
///
/// It is safe to call this function with interrupts enabled, but returned id may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn cpuid() -> usize {
    unsafe { r_tp() }
}

/// Return the current struct Proc *, or zero if none.
pub unsafe fn myproc() -> *mut Proc {
    push_off();
    let c = kernel().mycpu();
    let p = (*c).proc;
    pop_off();
    p
}

/// Free a proc structure and the data hanging from it,
/// including user pages.
/// p->lock must be held.
unsafe fn freeproc(mut p: ProcGuard) {
    let mut data = &mut *p.data.get();
    if !data.trapframe.is_null() {
        kernel().free(Page::from_usize(data.trapframe as _));
    }
    data.trapframe = ptr::null_mut();
    if !data.pagetable.is_null() {
        let sz = data.sz;
        proc_freepagetable(&mut data.pagetable, sz);
    }
    data.pagetable = PageTable::zero();
    data.sz = 0;
    p.deref_mut_info().pid = 0;
    p.deref_mut_info().parent = ptr::null_mut();
    (*p).name[0] = 0;
    p.deref_mut_info().waitchannel = ptr::null();
    p.killed = AtomicBool::new(false);
    p.deref_mut_info().xstate = 0;
    p.deref_mut_info().state = Procstate::UNUSED;
}

/// Create a user page table for a given process,
/// with no user memory, but with trampoline pages.
pub unsafe fn proc_pagetable(p: *mut Proc) -> PageTable<UVAddr> {
    // An empty page table.
    let mut pagetable = PageTable::<UVAddr>::zero();
    pagetable.alloc_root();

    // let mut pagetable = uvmcreate();

    // Map the trampoline code (for system call return)
    // at the highest user virtual address.
    // Only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    pagetable
        .mappages(
            UVAddr::new(TRAMPOLINE),
            PGSIZE,
            trampoline.as_mut_ptr() as usize,
            PTE_R | PTE_X,
        )
        .expect("proc_pagetable: mappages TRAMPOLINE");

    // Map the trapframe just below TRAMPOLINE, for trampoline.S.
    pagetable
        .mappages(
            UVAddr::new(TRAPFRAME),
            PGSIZE,
            (*(*p).data.get()).trapframe as usize,
            PTE_R | PTE_W,
        )
        .expect("proc_pagetable: mappages TRAPFRAME");
    pagetable
}

/// Free a process's page table, and free the
/// physical memory it refers to.
pub unsafe fn proc_freepagetable(pagetable: &mut PageTable<UVAddr>, sz: usize) {
    pagetable.uvmunmap(UVAddr::new(TRAMPOLINE), PGSIZE, 0);
    pagetable.uvmunmap(UVAddr::new(TRAPFRAME), PGSIZE, 0);
    pagetable.uvmfree(sz);
}

/// A user program that calls exec("/init").
/// od -t xC initcode
const INITCODE: [u8; 51] = [
    0x17, 0x05, 0, 0, 0x13, 0x05, 0x05, 0x02, 0x97, 0x05, 0, 0, 0x93, 0x85, 0x05, 0x02, 0x9d, 0x48,
    0x73, 0, 0, 0, 0x89, 0x48, 0x73, 0, 0, 0, 0xef, 0xf0, 0xbf, 0xff, 0x2f, 0x69, 0x6e, 0x69, 0x74,
    0, 0, 0x01, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Grow or shrink user memory by n bytes.
/// Return 0 on success, -1 on failure.
pub unsafe fn resizeproc(n: i32) -> i32 {
    let p = myproc();
    let data = &mut *(*p).data.get();
    let sz = data.sz;
    let sz = match n.cmp(&0) {
        cmp::Ordering::Equal => sz,
        cmp::Ordering::Greater => {
            let sz = data.pagetable.uvmalloc(sz, sz.wrapping_add(n as usize));
            ok_or!(sz, return -1)
        }
        cmp::Ordering::Less => data.pagetable.uvmdealloc(sz, sz.wrapping_add(n as usize)),
    };
    data.sz = sz;
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
    let mut c = kernel().mycpu();
    (*c).proc = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();

        for p in &kernel().procs.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().state == Procstate::RUNNABLE {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                guard.deref_mut_info().state = Procstate::RUNNING;
                (*c).proc = p as *const _ as *mut _;
                swtch(&mut (*c).context, &mut (*guard.data.get()).context);

                // Process is done running for now.
                // It should have changed its p->state before coming back.
                (*c).proc = ptr::null_mut()
            }
        }
    }
}

/// Give up the CPU for one scheduling round.
pub unsafe fn proc_yield() {
    let p = myproc();
    let mut guard = (*p).lock();
    guard.deref_mut_info().state = Procstate::RUNNABLE;
    guard.sched();
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
unsafe fn forkret() {
    // Still holding p->lock from scheduler.
    (*myproc()).info.unlock();

    // File system initialization must be run in the context of a
    // regular process (e.g., because it calls sleep), and thus cannot
    // be run from main().
    kernel().fsinit(ROOTDEV);

    usertrapret();
}
