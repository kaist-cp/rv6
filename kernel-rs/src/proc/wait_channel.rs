use super::*;
use crate::{
    kernel::KernelRef,
    lock::{Guard, RawLock},
};

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
        let mut guard = ctx.proc().lock();
        // Release the lock while we sleep on the waitchannel, and reacquire after the process wakes up.
        lock_guard.reacquire_after(move || {
            // Go to sleep.
            guard.deref_mut_info().waitchannel = self;
            guard.deref_mut_info().state = Procstate::SLEEPING;
            // SAFETY: we hold `p.lock()`, changed the process's state,
            // and device interrupts are disabled by `push_off()` in `p.lock()`.
            unsafe { guard.sched() };

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
