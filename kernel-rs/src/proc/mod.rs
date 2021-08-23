use core::{
    cell::UnsafeCell,
    mem::{self, MaybeUninit},
    ops::Deref,
    ptr, str,
    sync::atomic::{AtomicBool, Ordering},
};

use array_macro::array;

use crate::{
    arch::interface::{ContextManager, ProcManager, TrapManager},
    arch::TargetArch,
    file::RcFile,
    fs::{DefaultFs, RcInode},
    hal::hal,
    lock::SpinLock,
    page::Page,
    param::{MAXPROCNAME, NOFILE},
    util::branded::Branded,
    vm::UserMemory,
};

mod kernel_ctx;
mod procs;
mod wait_channel;

pub use kernel_ctx::*;
pub use procs::*;
pub use wait_channel::*;

type Context = <TargetArch as ProcManager>::Context;

extern "C" {
    // swtch.S
    fn swtch(_: *mut Context, _: *mut Context);
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

/// Proc::info's spinlock must be held when using these.
pub struct ProcInfo {
    /// Process state.
    pub state: Procstate,

    /// If non-zero, sleeping on waitchannel.
    waitchannel: *const WaitChannel,

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
    trap_frame: *mut <TargetArch as ProcManager>::TrapFrame,

    /// User memory manager
    memory: MaybeUninit<UserMemory>,

    /// swtch() here to run process.
    context: Context,

    /// Open files.
    pub open_files: [Option<RcFile>; NOFILE],

    /// Current directory.
    cwd: MaybeUninit<RcInode<DefaultFs>>,

    /// Process name (debugging).
    pub name: [u8; MAXPROCNAME],
}

/// Per-process state.
///
/// # Safety
///
/// * If `info.state` ≠ `UNUSED`, then
///   - `data.trap_frame` is a valid pointer, and `Page::from_usize(data.trap_frame)` is safe.
///   - `data.memory` has been initialized.
/// * If `info.state` ∉ { `UNUSED`, `USED` }, then
///   - `data.cwd` has been initialized.
///   - `parent` contains null or a valid pointer. `parent` can be null only when `self` is the same
///     as `initial_proc` of `Procs` that contains `self`.
pub struct Proc {
    /// Parent process.
    parent: UnsafeCell<*const Proc>,

    pub info: SpinLock<ProcInfo>,

    data: UnsafeCell<ProcData>,

    /// Waitchannel saying child proc is dead.
    child_waitchannel: WaitChannel,

    /// If true, the process have been killed.
    killed: AtomicBool,
}

/// A branded reference to a `Proc`.
/// For a `ProcsRef<'id, '_>` that has the same `'id` tag with this, the `Proc` is owned
/// by the `Procs` that the `ProcsRef` points to.
#[derive(Clone, Copy)]
pub struct ProcRef<'id, 's>(Branded<'id, &'s Proc>);

/// # Safety
///
/// * `proc.info` is locked.
pub struct ProcGuard<'id, 's> {
    proc: ProcRef<'id, 's>,
}

impl Procstate {
    fn as_str(&self) -> &'static str {
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
            memory: MaybeUninit::uninit(),
            context: Context::new(),
            open_files: array![_ => None; NOFILE],
            cwd: MaybeUninit::uninit(),
            name: [0; MAXPROCNAME],
        }
    }
}

impl Proc {
    const fn new() -> Self {
        Self {
            parent: UnsafeCell::new(ptr::null()),
            info: SpinLock::new(
                "proc",
                ProcInfo {
                    state: Procstate::UNUSED,
                    waitchannel: ptr::null(),
                    xstate: 0,
                    pid: 0,
                },
            ),
            data: UnsafeCell::new(ProcData::new()),
            child_waitchannel: WaitChannel::new(),
            killed: AtomicBool::new(false),
        }
    }
}

impl Proc {
    /// Kill and wake the process up.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Release);
    }

    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }
}

impl<'id, 's> ProcRef<'id, 's> {
    /// Returns a mutable reference to this `Proc`'s parent field, which is a raw pointer.
    /// You need a `WaitGuard` that has the same `'id`.
    fn get_mut_parent<'a: 'b, 'b>(
        &'a self,
        _guard: &'b mut WaitGuard<'id, '_>,
    ) -> &'b mut *const Proc {
        unsafe { &mut *self.parent.get() }
    }

    pub fn lock(&self) -> ProcGuard<'id, 's> {
        mem::forget(self.info.lock());
        ProcGuard { proc: *self }
    }
}

impl<'s> Deref for ProcRef<'_, 's> {
    type Target = Proc;

    fn deref(&self) -> &'s Self::Target {
        &self.0
    }
}

impl<'id> ProcGuard<'id, '_> {
    fn deref_info(&self) -> &ProcInfo {
        // SAFETY: self.info is locked.
        unsafe { &*self.info.get_mut_raw() }
    }

    fn deref_mut_info(&mut self) -> &mut ProcInfo {
        // SAFETY: self.info is locked and &mut self is exclusive.
        unsafe { &mut *self.info.get_mut_raw() }
    }

    /// This method returns a mutable reference to its `ProcData`. There is no
    /// data race between `ProcGuard`s since this method can be called only after
    /// acquiring the lock of `info`. However, `CurrentProc` can create a mutable
    /// reference to the `ProcData` without acquiring the lock. Therefore, this
    /// method is unsafe, and the caller must ensure the below safety condition.
    ///
    /// # Safety
    ///
    /// This method must be called only when there is no `CurrentProc` referring
    /// to the same `Proc`.
    unsafe fn deref_mut_data(&mut self) -> &mut ProcData {
        unsafe { &mut *self.data.get() }
    }

    /// Switch to scheduler.  Must hold only p->lock
    /// and have changed proc->state. Saves and restores
    /// interrupt_enabled because interrupt_enabled is a property of this
    /// kernel thread, not this CPU. It should
    /// be proc->interrupt_enabled and proc->noff, but that would
    /// break in the few places where a lock is held but
    /// there's no process.
    unsafe fn sched(&mut self) {
        assert!(!TargetArch::intr_get(), "sched interruptible");
        assert_ne!(self.state(), Procstate::RUNNING, "sched running");

        // SAFETY: interrupts are disabled.
        let cpu = unsafe { hal().get_ref().cpus().current_unchecked() };
        assert_eq!(cpu.get_noff(), 1, "sched locks");

        let interrupt_enabled = cpu.get_interrupt();
        unsafe { swtch(&mut self.deref_mut_data().context, cpu.context_raw_mut()) };

        // We cannot use `cpu` again because `swtch` may move this thread to another cpu.
        // SAFETY: interrupts are disabled.
        let cpu = unsafe { hal().get_ref().cpus().current_unchecked() };
        cpu.set_interrupt(interrupt_enabled);
    }

    /// Frees a `Proc` structure and the data hanging from it, including user pages.
    /// Also, clears `p`'s parent field into `ptr::null_mut()`.
    /// The caller must provide a `ProcGuard`.
    ///
    /// # Safety
    ///
    /// `self.info.state` ≠ `UNUSED`
    unsafe fn clear(&mut self, mut parent_guard: WaitGuard<'id, '_>) {
        // SAFETY: this process cannot be the current process any longer.
        let data = unsafe { self.deref_mut_data() };
        let trap_frame = mem::replace(&mut data.trap_frame, ptr::null_mut());
        let allocator = hal().kmem();
        // SAFETY: trap_frame uniquely refers to a valid page.
        allocator.free(unsafe { Page::from_usize(trap_frame as _) });
        // SAFETY:
        // * ok to assume_init() because memory has been initialized according to the invariant.
        // * ok to replace memory with uninit() because state will become UNUSED.
        unsafe {
            mem::replace(&mut data.memory, MaybeUninit::uninit())
                .assume_init()
                .free(allocator)
        };

        // Clear the name.
        data.name[0] = 0;

        // Clear the process's parent field.
        *self.get_mut_parent(&mut parent_guard) = ptr::null_mut();
        drop(parent_guard);

        // Clear the `ProcInfo`.
        let info = self.deref_mut_info();
        info.waitchannel = ptr::null();
        info.pid = 0;
        info.xstate = 0;
        info.state = Procstate::UNUSED;

        self.killed.store(false, Ordering::Release);
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

    fn reacquire_after<F, U>(&mut self, f: F) -> U
    where
        F: FnOnce(ProcRef<'id, '_>) -> U,
    {
        // SAFETY: releasing is temporal, and `self` as `ProcGuard` cannot be used in `f`.
        unsafe { self.info.unlock() };
        let result = f(**self);
        mem::forget(self.info.lock());
        result
    }
}

impl<'id, 's> Deref for ProcGuard<'id, 's> {
    type Target = ProcRef<'id, 's>;

    fn deref(&self) -> &Self::Target {
        &self.proc
    }
}

impl Drop for ProcGuard<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: self will be dropped.
        unsafe { self.info.unlock() };
    }
}

pub enum RegNum {
    R0,
    R1,
    R2,
    R3,
    R4,
    R5,
    R6,
    R7,
}

impl From<usize> for RegNum {
    fn from(item: usize) -> Self {
        match item {
            0 => Self::R0,
            1 => Self::R1,
            2 => Self::R2,
            3 => Self::R3,
            4 => Self::R4,
            5 => Self::R5,
            6 => Self::R6,
            7 => Self::R7,
            _ => panic!(),
        }
    }
}
