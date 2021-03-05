//! Sleeping locks
use core::cell::UnsafeCell;

use super::{Guard, Lock, RawLock, Sleepablelock};
use crate::kernel::kernel_builder;

/// Long-term locks for processes
pub struct RawSleeplock {
    /// Process holding lock. `-1` means unlocked.
    locked: Sleepablelock<i32>,

    /// Name of lock for debugging.
    name: &'static str,
}

/// Locks that sleep instead of busy wait.
pub type Sleeplock<T> = Lock<RawSleeplock, T>;
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
        *guard = kernel_builder()
            .current_proc()
            .expect("No current proc")
            .pid();
    }

    fn release(&self) {
        let mut guard = self.locked.lock();
        *guard = -1;
        guard.wakeup();
    }

    fn holding(&self) -> bool {
        let guard = self.locked.lock();
        *guard
            == kernel_builder()
                .current_proc()
                .expect("No current proc")
                .pid()
    }
}

impl<T> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    /// If `T: Unpin`, `Sleeplock::new` should be used instead.
    ///
    /// # Safety
    ///
    /// If `T: !Unpin`, `Sleeplock` or `SleeplockGuard` will only provide pinned mutable references
    /// of the inner data to the outside. However, it is still the caller's responsibility to
    /// make sure that the `Sleeplock` itself never gets moved.
    pub const unsafe fn new_unchecked(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: Unpin> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        // Safe since `T: Unpin`.
        unsafe { Self::new_unchecked(name, data) }
    }
}
