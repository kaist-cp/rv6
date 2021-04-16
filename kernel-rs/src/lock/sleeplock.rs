//! Sleeping locks
use core::cell::UnsafeCell;

use super::{Guard, Lock, RawLock, Sleepablelock};
use crate::proc::kernel_ctx;

/// Long-term locks for processes
pub struct RawSleeplock {
    /// Process holding lock. `-1` means unlocked.
    locked: Sleepablelock<i32>,

    /// Name of lock for debugging.
    name: &'static str,
}

/// Locks that sleep instead of busy wait.
pub type Sleeplock<T> = Lock<RawSleeplock, T>;
/// Guards of `Sleeplock<T>`.
pub type SleeplockGuard<'s, T> = Guard<'s, RawSleeplock, T>;

impl RawSleeplock {
    const fn new(name: &'static str) -> Self {
        Self {
            locked: Sleepablelock::new("sleep lock", -1),
            name,
        }
    }
}

impl RawLock for RawSleeplock {
    fn acquire(&self) {
        let mut guard = self.locked.lock();
        while *guard != -1 {
            guard.sleep();
        }
        // TODO: remove kernel_ctx()
        *guard = unsafe { kernel_ctx() }.proc.pid();
    }

    fn release(&self) {
        let mut guard = self.locked.lock();
        *guard = -1;
        guard.wakeup();
    }

    fn holding(&self) -> bool {
        let guard = self.locked.lock();
        // TODO: remove kernel_ctx()
        *guard == unsafe { kernel_ctx() }.proc.pid()
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
