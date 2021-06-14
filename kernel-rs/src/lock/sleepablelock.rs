//! Sleepable locks
use core::cell::UnsafeCell;

use super::{spinlock::RawSpinLock, Guard, Lock, RawLock};
use crate::{
    kernel::KernelRef,
    proc::{KernelCtx, WaitChannel},
};

/// Mutual exclusion spin locks that can sleep.
pub struct RawSleepableLock {
    lock: RawSpinLock,
    /// WaitChannel used to sleep/wakeup the lock's guard.
    waitchannel: WaitChannel,
}

/// Similar to `SpinLock`, but guards of this lock can sleep.
pub type SleepableLock<T> = Lock<RawSleepableLock, T>;
/// Guards of `SleepableLock<T>`. These guards can `sleep()`/`wakeup()`.
pub type SleepableLockGuard<'s, T> = Guard<'s, RawSleepableLock, T>;

impl RawSleepableLock {
    /// Mutual exclusion sleepable locks.
    const fn new(name: &'static str) -> Self {
        Self {
            lock: RawSpinLock::new(name),
            waitchannel: WaitChannel::new(),
        }
    }
}

impl RawLock for RawSleepableLock {
    fn acquire(&self) {
        self.lock.acquire();
    }

    fn release(&self) {
        self.lock.release();
    }
}

impl<T> SleepableLock<T> {
    /// Returns a new `SleepableLock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleepableLock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T> SleepableLockGuard<'_, T> {
    pub fn sleep(&mut self, ctx: &KernelCtx<'_, '_>) {
        self.lock.lock.waitchannel.sleep(self, ctx);
    }

    pub fn wakeup(&self, kernel: KernelRef<'_, '_>) {
        self.lock.lock.waitchannel.wakeup(kernel);
    }
}
