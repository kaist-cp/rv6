#![allow(clippy::unit_arg)]

use core::{
    cell::UnsafeCell,
    mem::{self, MaybeUninit},
    ops::Deref,
    pin::Pin,
    ptr, slice, str,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use array_macro::array;
use pin_project::pin_project;

use crate::{
    file::RcFile,
    fs::{Path, RcInode},
    kernel::{kernel, Kernel},
    memlayout::kstack,
    page::Page,
    param::{MAXPROCNAME, NOFILE, NPROC, ROOTDEV},
    println,
    riscv::{intr_get, intr_on, r_tp, PGSIZE},
    spinlock::{
        pop_off, push_off, RawSpinlock, Spinlock, SpinlockProtected, SpinlockProtectedGuard,
    },
    trap::usertrapret,
    vm::{Addr, UVAddr, UserMemory},
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
    pub proc: *const Proc,

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
pub struct TrapFrame {
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
    USED,
}

type Pid = i32;

/// Represents lock guards that can be slept in a `WaitChannel`.
pub trait Waitable {
    /// Releases the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    /// Also, do not access `self` until re-acquiring the lock with `raw_acquire()`.
    unsafe fn raw_release(&mut self);

    /// Acquires the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    unsafe fn raw_acquire(&mut self);
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
    pub fn sleep<T: Waitable>(&self, lk: &mut T, proc: &CurrentProc<'_>) {
        // Must acquire p->lock in order to
        // change p->state and then call sched.
        // Once we hold p->lock, we can be
        // guaranteed that we won't miss any wakeup
        // (wakeup locks p->lock),
        // so it's okay to release lk.

        //DOC: sleeplock1
        let mut guard = proc.lock();
        unsafe {
            // Temporarily release the inner `RawSpinlock`.
            // This is safe, since we don't access `lk` until re-acquiring the lock
            // at `lk.raw_acquire()`.
            lk.raw_release();
        }

        // Go to sleep.
        guard.deref_mut_info().waitchannel = self;
        guard.deref_mut_info().state = Procstate::SLEEPING;
        unsafe {
            // Safe since we hold `p.lock()`, changed the process's state,
            // and device interrupts are disabled by `push_off()` in `p.lock()`.
            guard.sched();
        }

        // Tidy up.
        guard.deref_mut_info().waitchannel = ptr::null();

        // Reacquire original lock.
        drop(guard);
        unsafe {
            // Safe since this is paired with a previous `lk.raw_release()`.
            lk.raw_acquire();
        }
    }

    /// Wake up all processes sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup(&self) {
        kernel().procs.wakeup_pool(self)
    }
}

/// Proc::info's spinlock must be held when using these.
pub struct ProcInfo {
    /// Process state.
    pub state: Procstate,

    /// If non-zero, sleeping on waitchannel.
    waitchannel: *const WaitChannel,

    /// Waitchannel saying child proc is dead.
    child_waitchannel: WaitChannel,

    /// Exit status to be returned to parent's wait.
    xstate: i32,

    /// Process ID.
    pid: Pid,
}

/// Proc::data are private to the process, so lock need not be held.
pub struct ProcData {
    /// Virtual address of kernel stack.
    pub kstack: usize,

    /// Data page for trampoline.S.
    trap_frame: *mut TrapFrame,

    /// User memory manager
    pub memory: UserMemory,

    /// swtch() here to run process.
    context: Context,

    /// Open files.
    pub open_files: [Option<RcFile<'static>>; NOFILE],

    /// Current directory.
    pub cwd: Option<RcInode<'static>>,

    /// Process name (debugging).
    pub name: [u8; MAXPROCNAME],
}

/// Per-process state.
///
/// # Safety
///
/// If info.state != UNUSED, then Page::from_usize(data.trap_frame) succeeds
/// without breaking the invariant of Page.
pub struct Proc {
    /// Parent process.
    ///
    /// We have to use a `MaybeUninit` type here, since we can't initialize
    /// this field in Proc::zero(), which is a const fn.
    /// Hence, this field gets initialized later in procinit() as
    /// `SpinlockProtected::new(&procs.wait_lock, ptr::null_mut())`.
    parent: MaybeUninit<SpinlockProtected<*const Proc>>,

    pub info: Spinlock<ProcInfo>,

    data: UnsafeCell<ProcData>,

    /// If true, the process have been killed.
    killed: AtomicBool,
}

/// CurrentProc wraps mutable pointer of current CPU's proc.
///
/// # Safety
///
/// `inner` is current Cpu's proc, this means it's state is `RUNNING`.
pub struct CurrentProc<'p> {
    inner: &'p Proc,
}

impl<'p> CurrentProc<'p> {
    /// # Safety
    ///
    /// `proc` should be current `Cpu`'s `proc`.
    unsafe fn new(proc: &'p Proc) -> Self {
        CurrentProc { inner: proc }
    }

    fn raw(&self) -> *const Proc {
        self.inner as *const Proc
    }

    pub fn pid(&self) -> Pid {
        // Safe because pid is not modified while CurrentProc exists.
        unsafe { (*self.info.get_mut_raw()).pid }
    }

    /// Give up the CPU for one scheduling round.
    pub unsafe fn proc_yield(&self) {
        let mut guard = self.lock();
        guard.deref_mut_info().state = Procstate::RUNNABLE;
        unsafe { guard.sched() };
    }

    pub fn deref_data_raw(&mut self) -> *mut ProcData {
        self.data.get()
    }

    pub fn deref_mut_data(&mut self) -> &mut ProcData {
        // Safety: Only `CurrentProc` can use `ProcData` without lock.
        unsafe { &mut *self.data.get() }
    }
}

impl Deref for CurrentProc<'_> {
    type Target = Proc;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

/// # Safety
///
/// (*ptr).info is locked.
pub struct ProcGuard {
    ptr: *const Proc,
}

impl ProcGuard {
    fn deref_info(&self) -> &ProcInfo {
        // It is safe becuase self.info is locked.
        unsafe { &*self.info.get_mut_raw() }
    }

    fn deref_mut_info(&mut self) -> &mut ProcInfo {
        // It is safe becuase self.info is locked and &mut self is exclusive.
        unsafe { &mut *self.info.get_mut_raw() }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn deref_mut_data(&self) -> &mut ProcData {
        // Safety: It is safe to access `ProcData` when lock is held.
        unsafe { &mut *self.data.get() }
    }

    unsafe fn from_raw(ptr: *const Proc) -> Self {
        Self { ptr }
    }

    fn raw(&self) -> *const Proc {
        self.ptr
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
        assert_ne!(self.state(), Procstate::RUNNING, "sched running");
        assert!(!intr_get(), "sched interruptible");

        let interrupt_enabled = unsafe { (*kernel().mycpu()).interrupt_enabled };
        unsafe {
            swtch(
                // TODO(#407): It is safe to use deref_mut_data because CurrentProc
                // is not used same time with context switch.
                &mut self.deref_mut_data().context,
                &mut (*kernel().mycpu()).context,
            )
        };
        unsafe { (*kernel().mycpu()).interrupt_enabled = interrupt_enabled };
    }

    /// Frees a `Proc` structure and the data hanging from it, including user pages.
    /// Must provide a `ProcGuard`, and optionally, you can also provide a `SpinlockProtectedGuard`
    /// if you also want to clear `p`'s parent field into `ptr::null_mut()`.
    ///
    /// # Note
    ///
    /// If a `SpinlockProtectedGuard` was not provided, `p`'s parent field is not modified.
    /// Note that this is because accessing a parent field without a `SpinlockProtectedGuard` is illegal.
    fn clear(&mut self, parent_guard: Option<SpinlockProtectedGuard<'_>>) {
        unsafe {
            // Clear the `ProcData`.
            // TODO(#407): It is safe to use deref_mut_data because we cleanup the `Proc` in this function.
            let data = self.deref_mut_data();
            let trap_frame = mem::replace(&mut data.trap_frame, ptr::null_mut());
            if !trap_frame.is_null() {
                kernel().free(Page::from_usize(trap_frame as _));
            }
            data.memory = UserMemory::uninit();

            // Clear the process's parent field.
            if let Some(mut guard) = parent_guard {
                *self.parent.assume_init_ref().get_mut(&mut guard) = ptr::null_mut();
            }

            // Clear the name.
            data.name[0] = 0;

            // Clear the `ProcInfo`.
            let info = self.deref_mut_info();
            info.waitchannel = ptr::null();
            info.pid = 0;
            info.xstate = 0;
            info.state = Procstate::UNUSED;

            self.killed.store(false, Ordering::Release);
        }
    }

    /// Wake process from sleep().
    fn wakeup(&mut self) {
        if self.state() == Procstate::SLEEPING {
            self.deref_mut_info().state = Procstate::RUNNABLE;
        }
    }

    pub fn state(&self) -> Procstate {
        self.deref_info().state
    }
}

impl Drop for ProcGuard {
    fn drop(&mut self) {
        unsafe {
            // If the ProcGuard was dropped while the process's state is still `USED`
            // and ProcData::sz == 0, this means an error happened while initializing a process.
            // Hence, clear the process's fields.
            if self.state() == Procstate::USED && self.memory.size() == 0 {
                self.clear(None);
            }
            self.info.unlock();
        }
    }
}

impl Deref for ProcGuard {
    type Target = Proc;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
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
            Procstate::USED => "used",
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
            trap_frame: ptr::null_mut(),
            memory: unsafe { UserMemory::uninit() },
            context: Context::new(),
            open_files: [None; NOFILE],
            cwd: None,
            name: [0; MAXPROCNAME],
        }
    }

    pub fn trap_frame(&self) -> &TrapFrame {
        unsafe { &*self.trap_frame }
    }

    pub fn trap_frame_mut(&mut self) -> &mut TrapFrame {
        unsafe { &mut *self.trap_frame }
    }

    /// Close all open files.
    unsafe fn close_files(&mut self) {
        for file in &mut self.open_files {
            *file = None;
        }
        // TODO(https://github.com/kaist-cp/rv6/issues/290)
        // If self.cwd is not None, the inode inside self.cwd will be dropped
        // by assigning None to self.cwd. Deallocation of an inode may cause
        // disk write operations, so we must begin a transaction here.
        let _tx = kernel().file_system.begin_transaction();
        self.cwd = None;
    }
}

/// TODO(https://github.com/kaist-cp/rv6/issues/363): pid, state, should be methods of ProcGuard.
impl Proc {
    const fn zero() -> Self {
        Self {
            parent: MaybeUninit::uninit(),
            info: Spinlock::new(
                "proc",
                ProcInfo {
                    state: Procstate::UNUSED,
                    child_waitchannel: WaitChannel::new(),
                    waitchannel: ptr::null(),
                    xstate: 0,
                    pid: 0,
                },
            ),
            data: UnsafeCell::new(ProcData::new()),
            killed: AtomicBool::new(false),
        }
    }

    pub fn lock(&self) -> ProcGuard {
        mem::forget(self.info.lock());
        ProcGuard { ptr: self }
    }

    /// Kill and wake the process up.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Release);
    }

    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }
}

impl Deref for Proc {
    type Target = ProcData;

    fn deref(&self) -> &Self::Target {
        // Safety: Only current proc uses ProcData.
        unsafe { &*self.data.get() }
    }
}

/// Process system type containing & managing whole processes.
#[pin_project]
pub struct ProcessSystem {
    nextpid: AtomicI32,
    process_pool: [Proc; NPROC],
    initial_proc: *const Proc,

    // Helps ensure that wakeups of wait()ing
    // parents are not lost. Helps obey the
    // memory model when using p->parent.
    // Must be acquired before any p->lock.
    wait_lock: RawSpinlock,
}

impl ProcessSystem {
    pub const fn zero() -> Self {
        Self {
            nextpid: AtomicI32::new(1),
            process_pool: array![_ => Proc::zero(); NPROC],
            initial_proc: ptr::null(),
            wait_lock: RawSpinlock::new("wait_lock"),
        }
    }

    /// Initialize the proc table at boot time.
    pub fn init(self: Pin<&'static mut Self>) {
        // Safe since we don't move the `ProcessSystem`.
        let this = unsafe { self.get_unchecked_mut() };
        for (i, p) in this.process_pool.iter_mut().enumerate() {
            let _ = p
                .parent
                .write(SpinlockProtected::new(&this.wait_lock, ptr::null_mut()));
            unsafe { &mut *p.data.get() }.kstack = kstack(i);
        }
    }

    fn allocpid(&self) -> Pid {
        self.nextpid.fetch_add(1, Ordering::Relaxed)
    }

    /// Look into process system for an UNUSED proc.
    /// If found, initialize state required to run in the kernel,
    /// and return with p->lock held.
    /// If there are no free procs, or a memory allocation fails, return Err.
    unsafe fn alloc(&self, trap_frame: Page, memory: UserMemory) -> Result<ProcGuard, ()> {
        for p in &self.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().state == Procstate::UNUSED {
                // TODO(#407): It is safe to use deref_mut_data because this process is yet UNUSED.
                let data = guard.deref_mut_data();

                // Initialize trap frame and page table.
                data.trap_frame = trap_frame.into_usize() as _;
                data.memory = memory;

                // Set up new context to start executing at forkret,
                // which returns to user space.
                data.context = Default::default();
                data.context.ra = forkret as usize;
                data.context.sp = data.kstack.wrapping_add(PGSIZE);

                let info = guard.deref_mut_info();
                info.pid = self.allocpid();
                info.state = Procstate::USED;

                return Ok(guard);
            }
        }

        kernel().free(trap_frame);
        Err(())
    }

    /// Pass p's abandoned children to init.
    /// Caller must provide a `SpinlockProtectedGuard`.
    unsafe fn reparent<'a: 'b, 'b>(
        &'a self,
        proc: *const Proc,
        parent_guard: &'b mut SpinlockProtectedGuard<'a>,
    ) {
        for pp in &self.process_pool {
            if *unsafe { pp.parent.assume_init_ref() }.get_mut(parent_guard) == proc {
                *unsafe { pp.parent.assume_init_ref() }.get_mut(parent_guard) = self.initial_proc;
                unsafe {
                    (*(*self.initial_proc).info.get_mut_raw())
                        .child_waitchannel
                        .wakeup()
                };
            }
        }
    }

    /// Kill the process with the given pid.
    /// The victim won't exit until it tries to return
    /// to user space (see usertrap() in trap.c).
    /// Returns Ok(()) on success, Err(()) on error.
    pub fn kill(&self, pid: Pid) -> Result<(), ()> {
        for p in &self.process_pool {
            let mut guard = p.lock();
            if guard.deref_info().pid == pid {
                p.kill();
                guard.wakeup();
                return Ok(());
            }
        }
        Err(())
    }

    /// Wake up all processes in the pool sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup_pool(&self, target: &WaitChannel) {
        let current_proc = kernel().current_proc().map_or(ptr::null(), |p| p.raw());
        for p in &self.process_pool {
            if p as *const Proc != current_proc {
                let mut guard = p.lock();
                if guard.deref_info().waitchannel == target as _ {
                    guard.wakeup()
                }
            }
        }
    }

    /// Set up first user process.
    pub unsafe fn user_proc_init(self: Pin<&mut Self>) {
        // Allocate trap frame.
        let trap_frame = scopeguard::guard(
            kernel().alloc().expect("user_proc_init: kernel().alloc"),
            |page| kernel().free(page),
        );

        // Allocate one user page and copy init's instructions
        // and data into it.
        let memory = UserMemory::new(trap_frame.addr(), Some(&INITCODE))
            .expect("user_proc_init: UserMemory::new");

        let mut guard =
            unsafe { self.alloc(scopeguard::ScopeGuard::into_inner(trap_frame), memory) }
                .expect("user_proc_init: ProcessSystem::alloc");

        *self.project().initial_proc = guard.raw() as *mut _;

        // TODO(#407): It is safe to use deref_mut_data because this process is not current process yet.
        let data = guard.deref_mut_data();

        // Prepare for the very first "return" from kernel to user.

        // User program counter.
        data.trap_frame_mut().epc = 0;

        // User stack pointer.
        data.trap_frame_mut().sp = PGSIZE;
        let name = b"initcode\x00";
        (&mut data.name[..name.len()]).copy_from_slice(name);
        data.cwd = Some(Path::root());
        guard.deref_mut_info().state = Procstate::RUNNABLE;
    }

    /// Create a new process, copying the parent.
    /// Sets up child kernel stack to return as if from fork() system call.
    /// Returns Ok(new process id) on success, Err(()) on error.
    pub unsafe fn fork(&self, proc: &mut CurrentProc<'_>) -> Result<Pid, ()> {
        // Allocate trap frame.
        let trap_frame = scopeguard::guard(kernel().alloc().ok_or(())?, |page| kernel().free(page));

        // Copy user memory from parent to child.
        let memory = proc
            .deref_mut_data()
            .memory
            .clone(trap_frame.addr())
            .ok_or(())?;

        // Allocate process.
        let mut np = unsafe { self.alloc(scopeguard::ScopeGuard::into_inner(trap_frame), memory) }?;
        // TODO(#407): It is safe to use deref_mut_data because this process is not current process yet.
        let npdata = np.deref_mut_data();

        // Copy saved user registers.
        *npdata.trap_frame_mut() = *proc.trap_frame();

        // Cause fork to return 0 in the child.
        npdata.trap_frame_mut().a0 = 0;

        // Increment reference counts on open file descriptors.
        for i in 0..NOFILE {
            if let Some(file) = &proc.open_files[i] {
                npdata.open_files[i] = Some(file.clone())
            }
        }
        npdata.cwd = Some(proc.cwd.clone().unwrap());

        npdata.name.copy_from_slice(&proc.name);

        let pid = np.deref_mut_info().pid;

        // Now drop the guard before we acquire the `wait_lock`.
        // This is because the lock order must be `wait_lock` -> `Proc::info`.
        let child = np.raw();
        drop(np);

        // Acquire the `wait_lock`, and write the parent field.
        let mut parent_guard = unsafe { (*child).parent.assume_init_ref().lock() };
        *unsafe { (*child).parent.assume_init_ref() }.get_mut(&mut parent_guard) = proc.raw();

        // Set the process's state to RUNNABLE.
        let mut np = unsafe { (*child).lock() };
        np.deref_mut_info().state = Procstate::RUNNABLE;

        Ok(pid)
    }

    /// Wait for a child process to exit and return its pid.
    /// Return Err(()) if this process has no children.
    pub unsafe fn wait(&self, addr: UVAddr, proc: &mut CurrentProc<'_>) -> Result<Pid, ()> {
        // Assumes that the process_pool has at least 1 element.
        let mut parent_guard = unsafe { self.process_pool[0].parent.assume_init_ref() }.lock();

        loop {
            // Scan through pool looking for exited children.
            let mut havekids = false;
            for np in &self.process_pool {
                if *unsafe { np.parent.assume_init_ref() }.get_mut(&mut parent_guard) == proc.raw()
                {
                    // Found a child.
                    // Make sure the child isn't still in exit() or swtch().
                    let mut np = np.lock();

                    havekids = true;
                    if np.state() == Procstate::ZOMBIE {
                        let pid = np.deref_mut_info().pid;
                        if !addr.is_null()
                            && proc
                                .deref_mut_data()
                                .memory
                                .copy_out(addr, unsafe {
                                    slice::from_raw_parts_mut(
                                        &mut np.deref_mut_info().xstate as *mut i32 as *mut u8,
                                        mem::size_of::<i32>(),
                                    )
                                })
                                .is_err()
                        {
                            return Err(());
                        }
                        // Reap the zombie child process.
                        np.clear(Some(parent_guard));
                        return Ok(pid);
                    }
                }
            }

            // No point waiting if we don't have any children.
            if !havekids || proc.killed() {
                return Err(());
            }

            // Wait for a child to exit.
            //DOC: wait-sleep
            unsafe {
                (*proc.info.get_mut_raw())
                    .child_waitchannel
                    .sleep(&mut parent_guard, proc)
            };
        }
    }

    /// Exit the current process.  Does not return.
    /// An exited process remains in the zombie state
    /// until its parent calls wait().
    pub unsafe fn exit_current(&self, status: i32, proc: &mut CurrentProc<'_>) -> ! {
        assert_ne!(proc.raw(), self.initial_proc, "init exiting");
        unsafe { proc.deref_mut_data().close_files() };

        // Give all children to init.
        let mut parent_guard = unsafe { proc.parent.assume_init_ref().lock() };
        unsafe { self.reparent(proc.raw(), &mut parent_guard) };

        // Parent might be sleeping in wait().
        unsafe {
            (*(**proc.parent.assume_init_ref().get_mut(&mut parent_guard))
                .info
                .get_mut_raw())
            .child_waitchannel
            .wakeup()
        };

        let mut guard = proc.lock();

        guard.deref_mut_info().xstate = status;
        guard.deref_mut_info().state = Procstate::ZOMBIE;

        // Should manually drop since this function never returns.
        drop(parent_guard);

        // Jump into the scheduler, and never return.
        unsafe { guard.sched() };

        unreachable!("zombie exit")
    }

    /// Print a process listing to the console for debugging.
    /// Runs when user types ^P on console.
    /// Doesn't acquire locks in order to avoid wedging a stuck machine further.
    pub unsafe fn dump(&self) {
        println!();
        for p in &self.process_pool {
            // For null character recognization.
            // Required since str::from_utf8 cannot recognize interior null characters.
            let length = p.name.iter().position(|&c| c == 0).unwrap_or(p.name.len());
            unsafe {
                let info = p.info.get_mut_raw();
                if (*info).state != Procstate::UNUSED {
                    println!(
                        "{} {} {}",
                        (*info).pid,
                        Procstate::to_str(&(*info).state),
                        str::from_utf8(&p.name[0..length]).unwrap_or("???")
                    );
                }
            }
        }
    }
}

/// Return this CPU's ID.
///
/// It is safe to call this function with interrupts enabled, but the returned id may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn cpuid() -> usize {
    r_tp()
}

/// A user program that calls exec("/init").
/// od -t xC initcode
const INITCODE: [u8; 52] = [
    0x17, 0x05, 0, 0, 0x13, 0x05, 0x45, 0x02, 0x97, 0x05, 0, 0, 0x93, 0x85, 0x35, 0x02, 0x93, 0x08,
    0x70, 0, 0x73, 0, 0, 0, 0x93, 0x08, 0x20, 0, 0x73, 0, 0, 0, 0xef, 0xf0, 0x9f, 0xff, 0x2f, 0x69,
    0x6e, 0x69, 0x74, 0, 0, 0x24, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Per-CPU process scheduler.
/// Each CPU calls scheduler() after setting itself up.
/// Scheduler never returns.  It loops, doing:
///  - choose a process to run.
///  - swtch to start running that process.
///  - eventually that process transfers control
///    via swtch back to the scheduler.
pub unsafe fn scheduler() -> ! {
    let mut c = kernel().mycpu();
    unsafe { (*c).proc = ptr::null_mut() };
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        unsafe { intr_on() };

        for p in &kernel().procs.process_pool {
            let mut guard = p.lock();
            if guard.state() == Procstate::RUNNABLE {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                guard.deref_mut_info().state = Procstate::RUNNING;
                unsafe { (*c).proc = p as *const _ };
                // TODO(#407): It is safe to use deref_mut_data because this process is not current process yet.
                unsafe { swtch(&mut (*c).context, &mut guard.deref_mut_data().context) };

                // Process is done running for now.
                // It should have changed its p->state before coming back.
                unsafe { (*c).proc = ptr::null_mut() }
            }
        }
    }
}

/// A fork child's very first scheduling by scheduler()
/// will swtch to forkret.
unsafe fn forkret() {
    let proc = &mut kernel().current_proc().expect("No current proc");
    // Still holding p->lock from scheduler.
    unsafe { proc.info.unlock() };

    // File system initialization must be run in the context of a
    // regular process (e.g., because it calls sleep), and thus cannot
    // be run from main().
    kernel().file_system.init(ROOTDEV);

    unsafe { usertrapret(proc) };
}

impl Kernel {
    /// Returns `Some<CurrentProc<'_>>` if current proc exists.
    /// If current proc is null, return `None`.
    /// If `(*c).proc` is non-null, returned `CurrentProc`'s `inner` lives during `&self`'s lifetime
    pub fn current_proc(&self) -> Option<CurrentProc<'_>> {
        unsafe { push_off() };
        let cpu = self.mycpu();
        let proc = unsafe { (*cpu).proc };
        unsafe { pop_off() };
        if proc.is_null() {
            return None;
        }
        // This is safe because p is non-null and current Cpu's proc.
        Some(unsafe { CurrentProc::new(&(*proc)) })
    }
}
