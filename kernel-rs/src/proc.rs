#![allow(clippy::unit_arg)]

use core::{
    cell::UnsafeCell,
    mem::{self, MaybeUninit},
    ops::Deref,
    pin::Pin,
    ptr, str,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use array_macro::array;
use itertools::izip;
use pin_project::pin_project;

use crate::{
    arch::addr::{Addr, UVAddr, PGSIZE},
    arch::memlayout::kstack,
    arch::riscv::{intr_get, intr_on, r_tp},
    file::RcFile,
    fs::RcInode,
    kalloc::Kmem,
    kernel::{kernel_builder, kernel_ref, KernelRef},
    lock::{pop_off, push_off, Guard, RawLock, RawSpinlock, RemoteLock, Spinlock, SpinlockGuard},
    page::Page,
    param::{MAXPROCNAME, NOFILE, NPROC, ROOTDEV},
    println,
    util::branded::Branded,
    vm::UserMemory,
};

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

/// Per-CPU-state.
#[derive(Copy, Clone)]
pub struct Cpu {
    /// The process running on this cpu, or null.
    proc: *const Proc,

    /// swtch() here to enter scheduler().
    context: Context,

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
    pub fn sleep<R: RawLock, T>(&self, lock_guard: &mut Guard<'_, R, T>, ctx: &KernelCtx<'_, '_>) {
        // Must acquire p->lock in order to
        // change p->state and then call sched.
        // Once we hold p->lock, we can be
        // guaranteed that we won't miss any wakeup
        // (wakeup locks p->lock),
        // so it's okay to release lk.

        //DOC: sleeplock1
        let mut guard = ctx.proc.lock();
        // Release the lock while we sleep on the waitchannel, and reacquire after the process wakes up.
        lock_guard.reacquire_after(move || {
            // Go to sleep.
            guard.deref_mut_info().waitchannel = self;
            guard.deref_mut_info().state = Procstate::SLEEPING;
            // SAFETY: we hold `p.lock()`, changed the process's state,
            // and device interrupts are disabled by `push_off()` in `p.lock()`.
            unsafe {
                guard.sched(ctx.kernel());
            }

            // Tidy up.
            guard.deref_mut_info().waitchannel = ptr::null();

            // Now we can drop the process guard since the process woke up.
            drop(guard);

            // Reacquire original lock.
        });
    }

    /// Wake up all processes sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup(&self, kernel: KernelRef<'_, '_>) {
        kernel.procs().wakeup_pool(self, kernel);
    }
}

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
    cwd: MaybeUninit<RcInode>,

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

/// Type that stores the context of the current thread. Consists of
/// * `KernelRef<'id, 'p>`, which points to the current kernel, and
/// * `CurrentProc<'id, 'p>`, which points to the current process.
///
/// # Note
///
/// We do not put `KernelRef` inside `CurrentProc` because we often need to access `KernelRef` while
/// mutably borrowing `CurrentProc`. Since `KernelRef` and `CurrentProc` are separate fields of
/// `KernelCtx`, we can separately borrow `Kernel` and `CurrentProc` when we use `KernelCtx`.
///
/// `KernelCtx` has `CurrentProc` instead of `Proc` because `CurrentProc` has more abilities than
/// `Proc`. `CurrentProc` allows accessing `ProcData` and the `MaybeUninit` fields, and `KernelCtx`
/// needs these abilities.
///
/// Methods that (possibly) need to access both `KernelRef` and `CurrentProc` take `KernelCtx` as
/// arguments. Otherwise, methods can take only one of `&Kernel` and `CurrentProc` as arguments.
pub struct KernelCtx<'id, 'p> {
    kernel: KernelRef<'id, 'p>,
    pub proc: CurrentProc<'id, 'p>,
}

impl<'id, 'p> KernelCtx<'id, 'p> {
    pub fn kernel(&self) -> KernelRef<'id, 'p> {
        self.kernel
    }

    pub fn proc(&self) -> &CurrentProc<'id, 'p> {
        &self.proc
    }

    pub fn proc_mut(&mut self) -> &mut CurrentProc<'id, 'p> {
        &mut self.proc
    }

    /// Give up the CPU for one scheduling round.
    // Its name cannot be `yield` because `yield` is a reserved keyword.
    pub fn yield_cpu(&self) {
        let mut guard = self.proc.lock();
        guard.deref_mut_info().state = Procstate::RUNNABLE;
        unsafe { guard.sched(self.kernel) };
    }
}

/// Creates the `KernelCtx` of the current Cpu.
/// The `KernelCtx` can be used only inside the given closure.
///
/// # Safety
///
/// * It must be called only after the initialization of the kernel.
/// * At most one `CurrentProc` or `KernelCtx` object can exist at a single time in each thread.
///   Therefore, it must not be called if the result of `current_proc` or `kernel_ctx` is alive.
pub unsafe fn kernel_ctx<'s, F: for<'new_id> FnOnce(KernelCtx<'new_id, 's>) -> R, R>(f: F) -> R {
    unsafe {
        kernel_ref(|kref| {
            let ctx = kref.get_ctx().expect("No current proc");
            f(ctx)
        })
    }
}

/// A branded reference to the current Cpu's `Proc`.
/// For a `ProcsRef<'id, '_>` that has the same `'id` tag, the `Proc` is owned by
/// the `Procs` that the `ProcsRef` points to.
///
/// # Safety
///
/// `inner` is the current Cpu's proc, whose state should be `RUNNING`.
pub struct CurrentProc<'id, 'p> {
    inner: Branded<'id, &'p Proc>,
}

impl<'id, 'p> CurrentProc<'id, 'p> {
    pub fn deref_data(&self) -> &ProcData {
        // SAFETY: Only `CurrentProc` can use `ProcData` without lock.
        unsafe { &*self.data.get() }
    }

    pub fn deref_mut_data(&mut self) -> &mut ProcData {
        // SAFETY: Only `CurrentProc` can use `ProcData` without lock.
        unsafe { &mut *self.data.get() }
    }

    pub fn pid(&self) -> Pid {
        // SAFETY: pid is not modified while CurrentProc exists.
        unsafe { (*self.info.get_mut_raw()).pid }
    }

    pub fn trap_frame(&self) -> &TrapFrame {
        // SAFETY: trap_frame is a valid pointer according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { &*self.deref_data().trap_frame }
    }

    pub fn trap_frame_mut(&mut self) -> &mut TrapFrame {
        // SAFETY: trap_frame is a valid pointer according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { &mut *self.deref_mut_data().trap_frame }
    }

    pub fn memory(&self) -> &UserMemory {
        // SAFETY: memory has been initialized according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { self.deref_data().memory.assume_init_ref() }
    }

    pub fn memory_mut(&mut self) -> &mut UserMemory {
        // SAFETY: memory has been initialized according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { self.deref_mut_data().memory.assume_init_mut() }
    }

    pub fn cwd(&self) -> &RcInode {
        // SAFETY: cwd has been initialized according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { self.deref_data().cwd.assume_init_ref() }
    }

    pub fn cwd_mut(&mut self) -> &mut RcInode {
        // SAFETY: cwd has been initialized according to the invariants
        // of ProcBuilder and CurrentProc.
        unsafe { self.deref_mut_data().cwd.assume_init_mut() }
    }
}

impl Deref for CurrentProc<'_, '_> {
    type Target = Proc;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// # Safety
///
/// * `proc.info` is locked.
pub struct ProcGuard<'s> {
    proc: &'s Proc,
}

impl ProcGuard<'_> {
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
    unsafe fn sched(&mut self, kernel: KernelRef<'_, '_>) {
        assert_eq!((*(kernel.current_cpu())).noff, 1, "sched locks");
        assert_ne!(self.state(), Procstate::RUNNING, "sched running");
        assert!(!intr_get(), "sched interruptible");

        let interrupt_enabled = unsafe { (*kernel.current_cpu()).interrupt_enabled };
        unsafe {
            swtch(
                &mut self.deref_mut_data().context,
                &mut (*kernel.current_cpu()).context,
            )
        };
        unsafe { (*kernel.current_cpu()).interrupt_enabled = interrupt_enabled };
    }

    /// Frees a `ProcBuilder` structure and the data hanging from it, including user pages.
    /// Also, clears `p`'s parent field into `ptr::null_mut()`.
    /// The caller must provide a `ProcGuard`.
    ///
    /// # Safety
    ///
    /// `self.info.state` ≠ `UNUSED`
    unsafe fn clear(&mut self, mut parent_guard: SpinlockGuard<'_, ()>) {
        // SAFETY: this process cannot be the current process any longer.
        let data = unsafe { self.deref_mut_data() };
        let trap_frame = mem::replace(&mut data.trap_frame, ptr::null_mut());
        // TODO: remove kernel_builder()
        let allocator = &kernel_builder().kmem;
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
        *self.parent().get_mut(&mut parent_guard) = ptr::null_mut();
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
        F: FnOnce(&Proc) -> U,
    {
        // SAFETY: releasing is temporal, and `self` as `ProcGuard` cannot be used in `f`.
        unsafe { self.info.unlock() };
        let result = f(&self);
        mem::forget(self.info.lock());
        result
    }
}

impl Drop for ProcGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: self will be dropped.
        unsafe { self.info.unlock() };
    }
}

impl Deref for ProcGuard<'_> {
    type Target = Proc;

    fn deref(&self) -> &Self::Target {
        self.proc
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

/// Process system type containing & managing whole processes.
///
/// # Safety
///
/// `initial_proc` is null or valid.
#[pin_project]
pub struct ProcsBuilder {
    nextpid: AtomicI32,
    #[pin]
    process_pool: [ProcBuilder; NPROC],
    initial_proc: *const Proc,

    // Helps ensure that wakeups of wait()ing
    // parents are not lost. Helps obey the
    // memory model when using p->parent.
    // Must be acquired before any p->lock.
    wait_lock: Spinlock<()>,
}

/// # Safety
///
/// `inner` has been initialized:
/// * `parent` of every `ProcBuilder` in `inner.process_pool` has been initialized.
/// * 'inner.wait_lock` must not be accessed.
#[repr(transparent)]
#[pin_project]
pub struct Procs {
    #[pin]
    inner: ProcsBuilder,
}

/// A branded reference to a `Procs`.
/// For a `KernelRef<'id, '_>` that has the same `'id` tag, the `Procs` is owned
/// by the `Kernel` that the `KernelRef` points to.
///
/// # Safety
///
/// A `ProcsRef<'id, 's>` can be created only from a `KernelRef<'id, 's>` that has the same `'id` tag.
pub struct ProcsRef<'id, 's>(Branded<'id, &'s Procs>);

impl Deref for ProcsRef<'_, '_> {
    type Target = Procs;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct ProcIter<'a> {
    iter: core::slice::Iter<'a, ProcBuilder>,
}

impl<'a> ProcIter<'a> {
    /// # Safety
    ///
    /// `parent` of every `ProcBuilder` in `iter` has been initialized.
    unsafe fn new(iter: core::slice::Iter<'a, ProcBuilder>) -> Self {
        Self { iter }
    }
}

impl<'a> Iterator for ProcIter<'a> {
    type Item = &'a Proc;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|inner: &'a ProcBuilder| unsafe { &*(inner as *const _ as *const _) })
    }
}

/// # Safety
///
/// `inner.parent` has been initialized.
#[repr(transparent)]
pub struct Proc {
    inner: ProcBuilder,
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

    pub fn lock(&self) -> ProcGuard<'_> {
        mem::forget(self.info.lock());
        ProcGuard { proc: self }
    }
}

impl Deref for Proc {
    type Target = ProcBuilder;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl ProcsBuilder {
    pub const fn zero() -> Self {
        Self {
            nextpid: AtomicI32::new(1),
            process_pool: array![_ => ProcBuilder::zero(); NPROC],
            initial_proc: ptr::null(),
            wait_lock: Spinlock::new("wait_lock", ()),
        }
    }

    /// Initialize the proc table at boot time.
    pub fn init(self: Pin<&mut Self>) -> Pin<&mut Procs> {
        // SAFETY: we don't move the `Procs`.
        let this = unsafe { self.get_unchecked_mut() };
        // SAFETY: we cast `wait_lock` to a raw pointer and cast again the raw pointer to a reference
        // because we want to return `self` from this method. The returned `self` is `Procs`, not
        // `ProcsBuilder`, and `Procs` disallows accessing `wait_lock` by its invariant. Therefore,
        // it's okay that both `&self` (for `wait_lock`) and `&mut self` (for the return value) are
        // alive at the same time.
        let wait_lock = unsafe { &*(&this.wait_lock as *const _) };
        for (i, p) in this.process_pool.iter_mut().enumerate() {
            let _ = p.parent.write(RemoteLock::new(wait_lock, ptr::null_mut()));
            p.data.get_mut().kstack = kstack(i);
        }
        // SAFETY: `parent` of every process in `self` has been initialized.
        let this = unsafe { this.as_procs_mut_unchecked() };
        // SAFETY: `this` has been pinned already.
        unsafe { Pin::new_unchecked(this) }
    }

    /// # Safety
    ///
    /// `parent` of every process in `self` must have been initialized.
    pub unsafe fn as_procs_unchecked(&self) -> &Procs {
        // SAFETY: `Procs` has a transparent memory layout, and `parent` of every process in `self`
        // has been initialized according to the safety condition of this method.
        unsafe { &*(self as *const _ as *const Procs) }
    }

    /// # Safety
    ///
    /// `parent` of every process in `self` must have been initialized.
    pub unsafe fn as_procs_mut_unchecked(&mut self) -> &mut Procs {
        // SAFETY: `Procs` has a transparent memory layout, and `parent` of every process in `self`
        // has been initialized according to the safety condition of this method.
        unsafe { &mut *(self as *mut _ as *mut Procs) }
    }
}

impl Procs {
    fn process_pool(&self) -> ProcIter<'_> {
        // SAFETY: invariant
        unsafe { ProcIter::new(self.inner.process_pool.iter()) }
    }

    fn initial_proc(&self) -> &Proc {
        assert!(!self.inner.initial_proc.is_null());
        // SAFETY: invariant
        unsafe { &*(self.inner.initial_proc as *const _) }
    }

    /// Look into process system for an UNUSED proc.
    /// If found, initialize state required to run in the kernel,
    /// and return with p->lock held.
    /// If there are no free procs, or a memory allocation fails, return Err.
    fn alloc(&self, trap_frame: Page, memory: UserMemory) -> Result<ProcGuard<'_>, ()> {
        for p in self.process_pool() {
            let mut guard = p.lock();
            if guard.deref_info().state == Procstate::UNUSED {
                // SAFETY: this process cannot be the current process yet.
                let data = unsafe { guard.deref_mut_data() };

                // Initialize trap frame and page table.
                data.trap_frame = trap_frame.into_usize() as _;
                let _ = data.memory.write(memory);

                // Set up new context to start executing at forkret,
                // which returns to user space.
                data.context = Default::default();
                data.context.ra = forkret as usize;
                data.context.sp = data.kstack + PGSIZE;

                let info = guard.deref_mut_info();
                info.pid = self.allocpid();
                // It's safe because trap_frame and memory now have been initialized.
                info.state = Procstate::USED;

                return Ok(guard);
            }
        }

        // TODO: remove kernel_builder()
        let allocator = &kernel_builder().kmem;
        allocator.free(trap_frame);
        memory.free(allocator);
        Err(())
    }

    fn allocpid(&self) -> Pid {
        self.inner.nextpid.fetch_add(1, Ordering::Relaxed)
    }

    /// Wake up all processes in the pool sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup_pool(&self, target: &WaitChannel, kernel: KernelRef<'_, '_>) {
        let current_proc = kernel.current_proc();
        for p in self.process_pool() {
            if p as *const _ != current_proc {
                let mut guard = p.lock();
                if guard.deref_info().waitchannel == target as _ {
                    guard.wakeup()
                }
            }
        }
    }

    /// Set up first user process.
    pub fn user_proc_init(self: Pin<&mut Self>, allocator: &Spinlock<Kmem>) {
        // Allocate trap frame.
        let trap_frame =
            scopeguard::guard(allocator.alloc().expect("user_proc_init: alloc"), |page| {
                allocator.free(page)
            });

        // Allocate one user page and copy init's instructions
        // and data into it.
        let memory = UserMemory::new(trap_frame.addr(), Some(&INITCODE), allocator)
            .expect("user_proc_init: UserMemory::new");

        let mut guard = self
            .alloc(scopeguard::ScopeGuard::into_inner(trap_frame), memory)
            .expect("user_proc_init: Procs::alloc");

        // SAFETY: this process cannot be the current process yet.
        let data = unsafe { guard.deref_mut_data() };

        // Prepare for the very first "return" from kernel to user.

        // User program counter.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { (*data.trap_frame).epc = 0 };

        // User stack pointer.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { (*data.trap_frame).sp = PGSIZE };

        let name = b"initcode\x00";
        (&mut data.name[..name.len()]).copy_from_slice(name);
        // TODO: remove kernel_builder()
        let _ = data.cwd.write(kernel_builder().itable.root());
        // It's safe because cwd now has been initialized.
        guard.deref_mut_info().state = Procstate::RUNNABLE;

        let initial_proc = guard.deref() as *const _;
        drop(guard);

        // It does not break the invariant since
        // * initial_proc is a pointer to a `Proc` inside self.
        // * self is pinned.
        *self.project().inner.project().initial_proc = initial_proc;
    }

    /// Pass p's abandoned children to init.
    /// Caller must provide a `SpinlockGuard`.
    fn reparent<'a: 'b, 'b>(
        &'a self,
        proc: *const Proc,
        parent_guard: &'b mut SpinlockGuard<'_, ()>,
        kernel: KernelRef<'_, '_>,
    ) {
        for pp in self.process_pool() {
            let parent = pp.parent().get_mut(parent_guard);
            if *parent == proc {
                *parent = self.initial_proc();
                self.initial_proc().child_waitchannel.wakeup(kernel);
            }
        }
    }

    /// Create a new process, copying the parent.
    /// Sets up child kernel stack to return as if from fork() system call.
    /// Returns Ok(new process id) on success, Err(()) on error.
    ///
    /// # Note
    ///
    /// `self` and `ctx` must have the same `'id` tag attached.
    /// Otherwise, UB may happen if the new `Proc` tries to read its `parent` field
    /// that points to a `Proc` that already dropped.
    pub fn fork<'id>(self: &ProcsRef<'id, '_>, ctx: &mut KernelCtx<'id, '_>) -> Result<Pid, ()> {
        let allocator = &ctx.kernel.kmem;
        // Allocate trap frame.
        let trap_frame =
            scopeguard::guard(allocator.alloc().ok_or(())?, |page| allocator.free(page));

        // Copy user memory from parent to child.
        let memory = ctx
            .proc
            .memory_mut()
            .clone(trap_frame.addr(), allocator)
            .ok_or(())?;

        // Allocate process.
        let mut np = self.alloc(scopeguard::ScopeGuard::into_inner(trap_frame), memory)?;
        // SAFETY: this process cannot be the current process yet.
        let npdata = unsafe { np.deref_mut_data() };

        // Copy saved user registers.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { *npdata.trap_frame = *ctx.proc.trap_frame() };

        // Cause fork to return 0 in the child.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { (*npdata.trap_frame).a0 = 0 };

        // Increment reference counts on open file descriptors.
        for (nf, f) in izip!(
            npdata.open_files.iter_mut(),
            ctx.proc.deref_data().open_files.iter()
        ) {
            if let Some(file) = f {
                *nf = Some(file.clone());
            }
        }
        let _ = npdata.cwd.write(ctx.proc.cwd_mut().clone());

        npdata.name.copy_from_slice(&ctx.proc.deref_data().name);

        let pid = np.deref_mut_info().pid;

        // Now drop the guard before we acquire the `wait_lock`.
        // This is because the lock order must be `wait_lock` -> `Proc::info`.
        np.reacquire_after(|np| {
            // Acquire the `wait_lock`, and write the parent field.
            let mut parent_guard = np.parent().lock();
            *np.parent().get_mut(&mut parent_guard) = ctx.proc.deref();
        });

        // Set the process's state to RUNNABLE.
        // It does not break the invariant because cwd now has been initialized.
        np.deref_mut_info().state = Procstate::RUNNABLE;

        Ok(pid)
    }

    /// Wait for a child process to exit and return its pid.
    /// Return Err(()) if this process has no children.
    pub fn wait<'id>(
        self: ProcsRef<'id, '_>,
        addr: UVAddr,
        ctx: &mut KernelCtx<'id, '_>,
    ) -> Result<Pid, ()> {
        // Assumes that the process_pool has at least 1 element.
        let some_proc = self.process_pool().next().unwrap();
        let mut parent_guard = some_proc.parent().lock();

        loop {
            // Scan through pool looking for exited children.
            let mut havekids = false;
            for np in self.process_pool() {
                if *np.parent().get_mut(&mut parent_guard) == ctx.proc.deref() {
                    // Found a child.
                    // Make sure the child isn't still in exit() or swtch().
                    let mut np = np.lock();

                    havekids = true;
                    if np.state() == Procstate::ZOMBIE {
                        let pid = np.deref_mut_info().pid;
                        if !addr.is_null()
                            && ctx
                                .proc
                                .memory_mut()
                                .copy_out(addr, &np.deref_info().xstate)
                                .is_err()
                        {
                            return Err(());
                        }
                        // Reap the zombie child process.
                        // SAFETY: np.state() equals ZOMBIE.
                        unsafe { np.clear(parent_guard) };
                        return Ok(pid);
                    }
                }
            }

            // No point waiting if we don't have any children.
            if !havekids || ctx.proc.killed() {
                return Err(());
            }

            // Wait for a child to exit.
            //DOC: wait-sleep
            ctx.proc.child_waitchannel.sleep(&mut parent_guard, ctx);
        }
    }

    /// Kill the process with the given pid.
    /// The victim won't exit until it tries to return
    /// to user space (see usertrap() in trap.c).
    /// Returns Ok(()) on success, Err(()) on error.
    pub fn kill(&self, pid: Pid) -> Result<(), ()> {
        for p in self.process_pool() {
            let mut guard = p.lock();
            if guard.deref_info().pid == pid {
                p.kill();
                guard.wakeup();
                return Ok(());
            }
        }
        Err(())
    }

    /// Exit the current process.  Does not return.
    /// An exited process remains in the zombie state
    /// until its parent calls wait().
    pub fn exit_current<'id>(
        self: &ProcsRef<'id, '_>,
        status: i32,
        ctx: &mut KernelCtx<'id, '_>,
    ) -> ! {
        assert_ne!(
            ctx.proc.deref() as *const _,
            self.initial_proc() as _,
            "init exiting"
        );

        for file in &mut ctx.proc.deref_mut_data().open_files {
            *file = None;
        }

        // TODO(https://github.com/kaist-cp/rv6/issues/290)
        // If self.cwd is not None, the inode inside self.cwd will be dropped
        // by assigning None to self.cwd. Deallocation of an inode may cause
        // disk write operations, so we must begin a transaction here.
        let kernel = ctx.kernel();
        let tx = kernel.file_system.begin_transaction();
        // SAFETY: CurrentProc's cwd has been initialized.
        // It's ok to drop cwd as proc will not be used any longer.
        unsafe { ctx.proc.deref_mut_data().cwd.assume_init_drop() };
        drop(tx);

        // Give all children to init.
        let mut parent_guard = ctx.proc.parent().lock();
        self.reparent(ctx.proc.deref(), &mut parent_guard, ctx.kernel());

        // Parent might be sleeping in wait().
        let parent = *ctx.proc.parent().get_mut(&mut parent_guard);
        // TODO: this assertion is actually unneccessary because parent is null
        // only when proc is the initial process, which cannot be the case.
        assert!(!parent.is_null());
        // SAFETY: parent is a valid pointer according to the invariants of
        // ProcBuilder and CurrentProc.
        unsafe { (*parent).child_waitchannel.wakeup(ctx.kernel()) };

        let mut guard = ctx.proc.lock();

        guard.deref_mut_info().xstate = status;
        guard.deref_mut_info().state = Procstate::ZOMBIE;

        // Should manually drop since this function never returns.
        drop(parent_guard);

        // Jump into the scheduler, and never return.
        unsafe { guard.sched(ctx.kernel()) };

        unreachable!("zombie exit")
    }

    /// Print a process listing to the console for debugging.
    /// Runs when user types ^P on console.
    /// Doesn't acquire locks in order to avoid wedging a stuck machine further.
    ///
    /// # Note
    ///
    /// This method is unsafe and should be used only for debugging.
    pub unsafe fn dump(&self) {
        println!();
        for p in self.process_pool() {
            let info = p.info.get_mut_raw();
            let state = unsafe { &(*info).state };
            if *state != Procstate::UNUSED {
                let name = unsafe { &(*p.data.get()).name };
                // For null character recognization.
                // Required since str::from_utf8 cannot recognize interior null characters.
                let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                println!(
                    "{} {} {}",
                    unsafe { (*info).pid },
                    Procstate::to_str(state),
                    str::from_utf8(&name[0..length]).unwrap_or("???")
                );
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

impl<'id, 's> KernelRef<'id, 's> {
    /// Per-CPU process scheduler.
    /// Each CPU calls scheduler() after setting itself up.
    /// Scheduler never returns.  It loops, doing:
    ///  - choose a process to run.
    ///  - swtch to start running that process.
    ///  - eventually that process transfers control
    ///    via swtch back to the scheduler.
    pub unsafe fn scheduler(self) -> ! {
        let mut cpu = self.current_cpu();
        unsafe { (*cpu).proc = ptr::null_mut() };
        loop {
            // Avoid deadlock by ensuring that devices can interrupt.
            unsafe { intr_on() };

            for p in self.procs().process_pool() {
                let mut guard = p.lock();
                if guard.state() == Procstate::RUNNABLE {
                    // Switch to chosen process.  It is the process's job
                    // to release its lock and then reacquire it
                    // before jumping back to us.
                    guard.deref_mut_info().state = Procstate::RUNNING;
                    unsafe { (*cpu).proc = p as *const _ };
                    unsafe { swtch(&mut (*cpu).context, &mut guard.deref_mut_data().context) };

                    // Process is done running for now.
                    // It should have changed its p->state before coming back.
                    unsafe { (*cpu).proc = ptr::null_mut() }
                }
            }
        }
    }

    /// Returns pointer to the current proc.
    fn current_proc(&self) -> *const Proc {
        unsafe { push_off() };
        let cpu = self.current_cpu();
        let proc = unsafe { (*cpu).proc };
        unsafe { pop_off() };
        proc
    }

    /// Returns `Some<KernelCtx<'id, '_>>` if current proc exists (i.e., when (*cpu).proc is non-null).
    /// Note that `'id` is same with the given `KernelRef`'s `'id`.
    /// Otherwise, returns `None` (when current proc is null).
    ///
    /// # Safety
    ///
    /// At most one `KernelCtx` object can exist at a single time in each thread.
    /// Therefore, it must not be called if the result of `kernel_ctx` is alive.
    pub unsafe fn get_ctx(self) -> Option<KernelCtx<'id, 's>> {
        let proc = self.current_proc();
        // This is safe because p is current Cpu's proc.
        let proc = unsafe { proc.as_ref() }?;
        Some(KernelCtx {
            kernel: self,
            proc: CurrentProc {
                inner: self.brand(proc),
            },
        })
    }

    /// Returns a `ProcsRef` that points to the kernel's `Procs`.
    pub fn procs(&self) -> ProcsRef<'id, '_> {
        ProcsRef(self.brand(self.deref().procs()))
    }
}

/// A fork child's very first scheduling by scheduler() will swtch to forkret.
unsafe fn forkret() {
    unsafe {
        kernel_ctx(|ctx| {
            // Still holding p->lock from scheduler.
            ctx.proc.info.unlock();
            // File system initialization must be run in the context of a
            // regular process (e.g., because it calls sleep), and thus cannot
            // be run from main().
            ctx.kernel.file_system.init(ROOTDEV, &ctx);
            ctx.user_trap_ret();
        })
    }
}
