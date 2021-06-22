use core::{
    marker::PhantomPinned,
    ops::Deref,
    pin::Pin,
    ptr, str,
    sync::atomic::{AtomicI32, Ordering},
};

use array_macro::array;
use itertools::izip;
use pin_project::pin_project;

use super::*;
use crate::{
    arch::addr::{Addr, UVAddr, PGSIZE},
    arch::memlayout::kstack,
    arch::riscv::intr_on,
    fs::FileSystem,
    hal::hal,
    kalloc::Kmem,
    kernel::KernelRef,
    lock::{SpinLock, SpinLockGuard},
    page::Page,
    param::{NPROC, ROOTDEV},
    util::branded::Branded,
    vm::UserMemory,
};

/// A user program that calls exec("/init").
/// od -t xC initcode
const INITCODE: [u8; 52] = [
    0x17, 0x05, 0, 0, 0x13, 0x05, 0x45, 0x02, 0x97, 0x05, 0, 0, 0x93, 0x85, 0x35, 0x02, 0x93, 0x08,
    0x70, 0, 0x73, 0, 0, 0, 0x93, 0x08, 0x20, 0, 0x73, 0, 0, 0, 0xef, 0xf0, 0x9f, 0xff, 0x2f, 0x69,
    0x6e, 0x69, 0x74, 0, 0, 0x24, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Process system type containing & managing whole processes.
///
/// # Safety
///
/// `initial_proc` is null or valid. `initial_proc` is not modified after its initialization in
/// `user_proc_init`.
#[pin_project]
pub struct Procs {
    nextpid: AtomicI32,
    #[pin]
    process_pool: [Proc; NPROC],
    initial_proc: *const Proc,
    // Helps ensure that wakeups of wait()ing
    // parents are not lost. Helps obey the
    // memory model when using p->parent.
    // Must be acquired before any p->lock.
    wait_lock: SpinLock<()>,
    #[pin]
    _marker: PhantomPinned,
}

/// A branded reference to a `Procs`.
/// For a `KernelRef<'id, '_>` that has the same `'id` tag with this, the `Procs` is owned
/// by the `Kernel` that the `KernelRef` points to.
///
/// # Safety
///
/// A `ProcsRef<'id, 's>` can be created only from a `KernelRef<'id, 's>` that has the same `'id` tag.
pub struct ProcsRef<'id, 's>(Branded<'id, Pin<&'s Procs>>);

struct ProcIter<'id, 'a>(Branded<'id, core::slice::Iter<'a, Proc>>);

/// A branded type that holds the guard of a `Procs::wait_lock`.
///
/// For a `ProcsRef<'id, '_>` that has the same `'id` tag with this, this `WaitGuard` acquires
/// the wait lock of the `Procs` that the `ProcsRef` points to.
///
/// To access the `parent` field of a `ProcRef<'id, '_>`, you need a `WaitGuard<'id, '_>`
/// with the same `'id` tag.
pub struct WaitGuard<'id, 's>(Branded<'id, SpinLockGuard<'s, ()>>);

impl Procs {
    pub const fn new() -> Self {
        Self {
            nextpid: AtomicI32::new(1),
            process_pool: array![_ => Proc::new(); NPROC],
            initial_proc: ptr::null(),
            wait_lock: SpinLock::new("wait_lock", ()),
            _marker: PhantomPinned,
        }
    }

    /// Initialize the proc table at boot time.
    pub fn init(self: Pin<&mut Self>) {
        // SAFETY: we don't move the `Procs`.
        let this = unsafe { self.get_unchecked_mut() };
        for (i, p) in this.process_pool.iter_mut().enumerate() {
            p.data.get_mut().kstack = kstack(i);
        }
    }

    /// Set up first user process.
    pub fn user_proc_init(
        self: Pin<&mut Self>,
        cwd: RcInode<<Ufs as FileSystem>::InodeInner>,
        allocator: Pin<&SpinLock<Kmem>>,
    ) {
        let initial_proc = Branded::new(self.as_ref(), |procs| {
            let procs = ProcsRef(procs);

            // Allocate trap frame.
            let trap_frame =
                scopeguard::guard(allocator.alloc().expect("user_proc_init: alloc"), |page| {
                    allocator.free(page)
                });

            // Allocate one user page and copy init's instructions
            // and data into it.
            let memory = UserMemory::new(trap_frame.addr(), Some(&INITCODE), allocator)
                .expect("user_proc_init: UserMemory::new");

            let mut guard = procs
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
            let _ = data.cwd.write(cwd);
            // It's safe because cwd now has been initialized.
            guard.deref_mut_info().state = Procstate::RUNNABLE;

            guard.deref().deref() as *const _
        });

        // It does not break the invariant since
        // * initial_proc is a pointer to a `Proc` inside self.
        // * self is pinned.
        *self.project().initial_proc = initial_proc;
    }

    fn initial_proc(self: Pin<&Self>) -> &Proc {
        assert!(!self.initial_proc.is_null());
        // SAFETY: invariant
        unsafe { &*(self.initial_proc as *const _) }
    }

    fn allocpid(self: Pin<&Self>) -> Pid {
        self.nextpid.fetch_add(1, Ordering::Relaxed)
    }
}

impl<'id, 's> ProcsRef<'id, 's> {
    fn process_pool(&self) -> ProcIter<'id, 's> {
        ProcIter::new(self)
    }

    /// Acquires the wait_lock of this `Procs` and returns the `WaitGuard`.
    /// You can access any of this `Procs`'s `Proc::parent` field only after acquiring the `WaitGuard`.
    fn wait_guard(&self) -> WaitGuard<'id, 's> {
        WaitGuard(self.0.brand(self.0.get_ref().wait_lock.lock()))
    }

    /// Look into process system for an UNUSED proc.
    /// If found, initialize state required to run in the kernel,
    /// and return with p->lock held.
    /// If there are no free procs, or a memory allocation fails, return Err.
    fn alloc(&self, trap_frame: Page, memory: UserMemory) -> Result<ProcGuard<'id, '_>, ()> {
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
                info.pid = self.0.allocpid();
                // It's safe because trap_frame and memory now have been initialized.
                info.state = Procstate::USED;

                return Ok(guard);
            }
        }

        let allocator = hal().kmem();
        allocator.free(trap_frame);
        memory.free(allocator);
        Err(())
    }

    /// Wake up all processes in the pool sleeping on waitchannel.
    /// Must be called without any p->lock.
    pub fn wakeup_pool(&self, target: &WaitChannel, kernel: KernelRef<'_, '_>) {
        let current_proc = kernel.current_proc();
        for p in self.process_pool() {
            if p.deref() as *const _ != current_proc {
                let mut guard = p.lock();
                if guard.deref_info().waitchannel == target as _ {
                    guard.wakeup()
                }
            }
        }
    }

    /// Pass p's abandoned children to init.
    /// Caller must provide a `SpinLockGuard`.
    fn reparent<'a: 'b, 'b>(
        &'a self,
        proc: *const Proc,
        parent_guard: &'b mut WaitGuard<'id, '_>,
        kernel: KernelRef<'_, '_>,
    ) {
        for pp in self.process_pool() {
            let parent = pp.get_mut_parent(parent_guard);
            if *parent == proc {
                *parent = self.0.initial_proc();
                self.0.initial_proc().child_waitchannel.wakeup(kernel);
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
    pub fn fork(&self, ctx: &mut KernelCtx<'id, '_>) -> Result<Pid, ()> {
        let allocator = hal().kmem();
        // Allocate trap frame.
        let trap_frame =
            scopeguard::guard(allocator.alloc().ok_or(())?, |page| allocator.free(page));

        // Copy user memory from parent to child.
        let memory = ctx
            .proc_mut()
            .memory_mut()
            .clone(trap_frame.addr(), allocator)
            .ok_or(())?;

        // Allocate process.
        let mut np = self.alloc(scopeguard::ScopeGuard::into_inner(trap_frame), memory)?;
        // SAFETY: this process cannot be the current process yet.
        let npdata = unsafe { np.deref_mut_data() };

        // Copy saved user registers.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { *npdata.trap_frame = *ctx.proc().trap_frame() };

        // Cause fork to return 0 in the child.
        // SAFETY: trap_frame has been initialized by alloc.
        unsafe { (*npdata.trap_frame).a0 = 0 };

        // Increment reference counts on open file descriptors.
        for (nf, f) in izip!(
            npdata.open_files.iter_mut(),
            ctx.proc().deref_data().open_files.iter()
        ) {
            if let Some(file) = f {
                *nf = Some(file.clone());
            }
        }
        let _ = npdata.cwd.write(ctx.proc().cwd().clone());

        npdata.name.copy_from_slice(&ctx.proc().deref_data().name);

        let pid = np.deref_mut_info().pid;

        // Now drop the guard before we acquire the `wait_lock`.
        // This is because the lock order must be `wait_lock` -> `Proc::info`.
        np.reacquire_after(|np| {
            // Acquire the `wait_lock`, and write the parent field.
            let mut parent_guard = self.wait_guard();
            *np.get_mut_parent(&mut parent_guard) = ctx.proc().deref().deref();
        });

        // Set the process's state to RUNNABLE.
        // It does not break the invariant because cwd now has been initialized.
        np.deref_mut_info().state = Procstate::RUNNABLE;

        Ok(pid)
    }

    /// Wait for a child process to exit and return its pid.
    /// Return Err(()) if this process has no children.
    pub fn wait(&self, addr: UVAddr, ctx: &mut KernelCtx<'id, '_>) -> Result<Pid, ()> {
        let mut parent_guard = self.wait_guard();

        loop {
            // Scan through pool looking for exited children.
            let mut havekids = false;
            for np in self.process_pool() {
                if *np.get_mut_parent(&mut parent_guard) == ctx.proc().deref().deref() {
                    // Found a child.
                    // Make sure the child isn't still in exit() or swtch().
                    let mut np = np.lock();

                    havekids = true;
                    if np.state() == Procstate::ZOMBIE {
                        let pid = np.deref_mut_info().pid;
                        if !addr.is_null()
                            && ctx
                                .proc_mut()
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
            if !havekids || ctx.proc().killed() {
                return Err(());
            }

            // Wait for a child to exit.
            //DOC: wait-sleep
            ctx.proc().child_waitchannel.sleep(&mut parent_guard.0, ctx);
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
    pub fn exit_current(&self, status: i32, ctx: &mut KernelCtx<'id, '_>) -> ! {
        assert_ne!(
            ctx.proc().deref().deref() as *const _,
            self.0.initial_proc() as _,
            "init exiting"
        );

        for i in 0..NOFILE {
            let files = &mut ctx.proc_mut().deref_mut_data().open_files;
            if let Some(f) = unsafe { files.get_unchecked_mut(i) }.take() {
                f.free(ctx);
            }
        }

        let tx = ctx.kernel().fs().as_pin().get_ref().begin_tx(ctx);
        // SAFETY:
        // * CurrentProc's cwd has been initialized.
        // * It's ok to take cwd because proc will not be used any longer.
        let cwd = unsafe { ctx.proc_mut().deref_mut_data().cwd.assume_init_read() };
        cwd.free((&tx, ctx));
        tx.end(ctx);

        // Give all children to init.
        let mut parent_guard = self.wait_guard();
        self.reparent(ctx.proc().deref().deref(), &mut parent_guard, ctx.kernel());

        // Parent might be sleeping in wait().
        let parent = *ctx.proc().get_mut_parent(&mut parent_guard);
        // SAFETY:
        // * `parent` cannot be null because it is not the initial process.
        // * `parent` is a valid pointer according to the invariants of
        //   `Proc` and `CurrentProc`.
        unsafe { (*parent).child_waitchannel.wakeup(ctx.kernel()) };

        let mut guard = ctx.proc().lock();

        guard.deref_mut_info().xstate = status;
        guard.deref_mut_info().state = Procstate::ZOMBIE;

        // Should manually drop since this function never returns.
        drop(parent_guard);

        // Jump into the scheduler, and never return.
        unsafe { guard.sched() };

        unreachable!("zombie exit")
    }
}

impl Deref for ProcsRef<'_, '_> {
    type Target = Procs;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A fork child's very first scheduling by scheduler() will swtch to forkret.
unsafe fn forkret() -> ! {
    let forkret_inner = |ctx: KernelCtx<'_, '_>| {
        // Still holding p->lock from scheduler.
        unsafe { ctx.proc().info.unlock() };
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        ctx.kernel().fs().init(ROOTDEV, &ctx);
        unsafe { ctx.user_trap_ret() }
    };

    unsafe { kernel_ctx(forkret_inner) }
}

impl<'id, 's> ProcIter<'id, 's> {
    fn new(procs: &ProcsRef<'id, 's>) -> Self {
        Self(procs.0.brand(procs.0.get_ref().process_pool.iter()))
    }
}

impl<'id, 'a> Iterator for ProcIter<'id, 'a> {
    type Item = ProcRef<'id, 'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|inner| ProcRef(self.0.brand(inner)))
    }
}

impl<'id, 's> KernelRef<'id, 's> {
    /// Returns a `ProcsRef` that points to the kernel's `Procs`.
    pub fn procs(&self) -> ProcsRef<'id, '_> {
        ProcsRef(self.brand(self.ps()))
    }

    /// Per-CPU process scheduler.
    /// Each CPU calls scheduler() after setting itself up.
    /// Scheduler never returns.  It loops, doing:
    ///  - choose a process to run.
    ///  - swtch to start running that process.
    ///  - eventually that process transfers control
    ///    via swtch back to the scheduler.
    pub unsafe fn scheduler(self) -> ! {
        // SAFETY: this function never moves to another CPU.
        let cpu = unsafe { hal().get_ref().cpus().current_unchecked() };
        cpu.set_proc(ptr::null_mut());
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
                    cpu.set_proc(p.deref());
                    unsafe { swtch(cpu.context_raw_mut(), &mut guard.deref_mut_data().context) };

                    // Process is done running for now.
                    // It should have changed its p->state before coming back.
                    cpu.set_proc(ptr::null_mut());
                }
            }
        }
    }

    /// Print a process listing to the console for debugging.
    /// Runs when user types ^P on console.
    /// Doesn't acquire locks in order to avoid wedging a stuck machine further.
    ///
    /// # Note
    ///
    /// This method is unsafe and should be used only for debugging.
    pub unsafe fn dump(&self) {
        self.as_ref().write_str("\n");
        for p in self.procs().process_pool() {
            let info = p.info.get_mut_raw();
            let state = unsafe { &(*info).state };
            if *state != Procstate::UNUSED {
                let name = unsafe { &(*p.data.get()).name };
                // For null character recognization.
                // Required since str::from_utf8 cannot recognize interior null characters.
                let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                self.as_ref().write_fmt(format_args!(
                    "{} {} {}",
                    unsafe { (*info).pid },
                    Procstate::as_str(state),
                    str::from_utf8(&name[0..length]).unwrap_or("???")
                ));
            }
        }
    }
}
