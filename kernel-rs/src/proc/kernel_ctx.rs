use core::ops::Deref;

use super::*;
use crate::kernel::{kernel_ref, KernelRef};

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
    proc: CurrentProc<'id, 'p>,
}

/// A branded reference to the current Cpu's `Proc`.
/// For a `ProcsRef<'id, '_>` that has the same `'id` tag, the `Proc` is owned by
/// the `Procs` that the `ProcsRef` points to.
///
/// # Safety
///
/// `inner` is the current Cpu's proc, whose state should be `RUNNING`.
pub struct CurrentProc<'id, 'p> {
    inner: ProcRef<'id, 'p>,
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
        unsafe { guard.sched() };
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

impl<'id, 'p> CurrentProc<'id, 'p> {
    pub fn pid(&self) -> Pid {
        // SAFETY: pid is not modified while CurrentProc exists.
        unsafe { (*self.info.get_mut_raw()).pid }
    }
}

impl<'id, 's> Deref for CurrentProc<'id, 's> {
    type Target = ProcRef<'id, 's>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'id, 's> KernelRef<'id, 's> {
    /// Returns pointer to the current proc.
    pub fn current_proc(&self) -> *const Proc {
        let cpus = hal().get_ref().cpus();
        let intr = cpus.push_off();
        let cpu = cpus.current(&intr);
        let proc = cpu.get_proc();
        unsafe { cpus.pop_off(intr) };
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
                inner: ProcRef(self.brand(proc)),
            },
        })
    }
}
