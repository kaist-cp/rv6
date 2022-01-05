//! Sleepable locks
use kernel_aam::lock::{Guard, Lock, RawLock};

use super::spinlock::RawSpinLock;
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

/// Returns a new `SleepableLock` with name `name` and data `data`.
pub const fn new_sleepable_lock<T>(name: &'static str, data: T) -> SleepableLock<T> {
    SleepableLock::new(RawSleepableLock::new(name), data)
}

pub fn sleep_guard<T>(this: &mut SleepableLockGuard<'_, T>, ctx: &KernelCtx<'_, '_>) {
    this.get_lock().raw_lock().waitchannel.sleep(this, ctx);
}

pub fn wakeup_guard<T>(this: &mut SleepableLockGuard<'_, T>, kernel: KernelRef<'_, '_>) {
    this.get_lock().raw_lock().waitchannel.wakeup(kernel);
}
