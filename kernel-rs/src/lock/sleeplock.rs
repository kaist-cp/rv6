//! Sleeping locks
use core::cell::UnsafeCell;

use super::{Guard, Lock, RawLock, Sleepablelock};
use crate::{kernel::kernel_ref, proc::kernel_ctx};

/// Long-term locks for processes
pub struct RawSleeplock {
    /// Process holding lock. `-1` means unlocked.
    inner: Sleepablelock<i32>,
}

/// Locks that sleep instead of busy wait.
pub type Sleeplock<T> = Lock<RawSleeplock, T>;
/// Guards of `Sleeplock<T>`.
pub type SleeplockGuard<'s, T> = Guard<'s, RawSleeplock, T>;

impl RawSleeplock {
    const fn new(name: &'static str) -> Self {
        Self {
            inner: Sleepablelock::new(name, -1),
        }
    }
}

impl RawLock for RawSleeplock {
    fn acquire(&self) {
        let mut guard = self.inner.lock();
        while *guard != -1 {
            // TODO(https://github.com/kaist-cp/rv6/issues/539): remove kernel_ctx()
            unsafe { kernel_ctx(|ctx| guard.sleep(&ctx)) };
        }
        // TODO(https://github.com/kaist-cp/rv6/issues/539): remove kernel_ctx()
        *guard = unsafe { kernel_ctx(|ctx| ctx.proc().pid()) };
    }

    fn release(&self) {
        let mut guard = self.inner.lock();
        *guard = -1;
        // TODO(https://github.com/kaist-cp/rv6/issues/539): remove kernel_ref()
        unsafe { kernel_ref(|kref| guard.wakeup(kref)) };
    }
}

impl<T> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}
