//! Sleepable locks
use core::cell::UnsafeCell;

use super::{spinlock::RawSpinlock, Guard, Lock, RawLock};
use crate::{
    kernel::KernelRef,
    proc::{KernelCtx, WaitChannel},
};

/// Mutual exclusion spin locks that can sleep.
pub struct RawSleepablelock {
    lock: RawSpinlock,
    /// WaitChannel used to sleep/wakeup the lock's guard.
    waitchannel: WaitChannel,
}

/// Similar to `Spinlock`, but guards of this lock can sleep.
pub type Sleepablelock<T> = Lock<RawSleepablelock, T>;
/// Guards of `Sleepablelock<T>`. These guards can `sleep()`/`wakeup()`.
pub type SleepablelockGuard<'s, T> = Guard<'s, RawSleepablelock, T>;

impl RawSleepablelock {
    /// Mutual exclusion sleepable locks.
    const fn new(name: &'static str) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            waitchannel: WaitChannel::new(),
        }
    }
}

impl RawLock for RawSleepablelock {
    fn acquire(&self) {
        self.lock.acquire();
    }

    fn release(&self) {
        self.lock.release();
    }
}

impl<T> Sleepablelock<T> {
    /// Returns a new `Sleepablelock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleepablelock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T> SleepablelockGuard<'_, T> {
    pub fn sleep(&mut self, ctx: &KernelCtx<'_, '_>) {
        self.lock.lock.waitchannel.sleep(self, ctx);
    }

    pub fn wakeup(&self, kernel: KernelRef<'_, '_>) {
        self.lock.lock.waitchannel.wakeup(kernel);
    }
}
