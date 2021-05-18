use core::{
    cell::UnsafeCell,
    mem::{self, MaybeUninit},
    ops::Deref,
    ptr, str,
    sync::atomic::{AtomicBool, Ordering},
};

use array_macro::array;

use crate::{
    arch::riscv::intr_get,
    file::RcFile,
    fs::{FileSystem, RcInode, Ufs},
    hal::hal,
    lock::{RawSpinlock, RemoteLock, Spinlock},
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

extern "C" {
    // swtch.S
    fn swtch(_: *mut Context, _: *mut Context);
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

/// ProcBuilder::info's spinlock must be held when using these.
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

/// ProcBuilder::data are private to the process, so lock need not be held.
pub struct ProcData {
    /// Virtual address of kernel stack.
    pub kstack: usize,

    /// Data page for trampoline.S.
    trap_frame: *mut TrapFrame,

    /// User memory manager
    memory: MaybeUninit<UserMemory>,

    /// swtch() here to run process.
    context: Context,

    /// Open files.
    pub open_files: [Option<RcFile>; NOFILE],

    /// Current directory.
    cwd: MaybeUninit<RcInode<<Ufs as FileSystem>::InodeInner>>,

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
///   - `parent` contains null or a valid pointer if it has been initialized.
pub struct ProcBuilder {
    /// Parent process.
    ///
    /// We have to use a `MaybeUninit` type here, since we can't initialize
    /// this field in ProcBuilder::zero(), which is a const fn.
    /// Hence, this field gets initialized later in procinit() as
    /// `RemoteSpinlock::new(&procs.wait_lock, ptr::null_mut())`.
    parent: MaybeUninit<RemoteLock<'static, RawSpinlock, (), *const Proc>>,

    pub info: Spinlock<ProcInfo>,

    data: UnsafeCell<ProcData>,

    /// Waitchannel saying child proc is dead.
    child_waitchannel: WaitChannel,

    /// If true, the process have been killed.
    killed: AtomicBool,
}

/// # Safety
///
/// `inner.parent` has been initialized.
#[repr(transparent)]
pub struct Proc {
    inner: ProcBuilder,
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

impl Context {
    pub const fn new() -> Self {
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

/// TODO(https://github.com/kaist-cp/rv6/issues/363): pid, state, should be methods of ProcGuard.
impl ProcBuilder {
    const fn zero() -> Self {
        Self {
            parent: MaybeUninit::uninit(),
            info: Spinlock::new(
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
    fn parent(&self) -> &RemoteLock<'static, RawSpinlock, (), *const Proc> {
        // SAFETY: invariant
        unsafe { self.parent.assume_init_ref() }
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
    type Target = ProcBuilder;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'id, 's> ProcRef<'id, 's> {
    /// Returns a mutable reference to this `Proc`'s parent field, which is a raw pointer.
    /// You need a `WaitGuard` that has the same `'id`.
    fn get_mut_parent<'a: 'b, 'b>(
        &'a self,
        guard: &'b mut WaitGuard<'id, '_>,
    ) -> &'b mut *const Proc {
        unsafe { self.parent().get_mut_unchecked(guard.get_mut_inner()) }
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
    /// to the same `ProcBuilder`.
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
        assert!(!intr_get(), "sched interruptible");
        assert_ne!(self.state(), Procstate::RUNNING, "sched running");

        let cpu = unsafe { &mut *hal().cpus.current() };
        assert_eq!(cpu.noff(), 1, "sched locks");

        let interrupt_enabled = cpu.get_interrupt();
        unsafe { swtch(&mut self.deref_mut_data().context, &mut cpu.context) };

        // We cannot use `cpu` again because `swtch` may move this thread to another cpu.
        let cpu = unsafe { &mut *hal().cpus.current() };
        cpu.set_interrupt(interrupt_enabled);
    }

    /// Frees a `ProcBuilder` structure and the data hanging from it, including user pages.
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
        let allocator = &hal().kmem;
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
