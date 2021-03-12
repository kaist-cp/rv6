//! Sleepable locks
use core::cell::UnsafeCell;

use super::{spinlock::RawSpinlock, Guard, Lock, RawLock, Waitable};
use crate::{kernel::kernel_builder, proc::WaitChannel};

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

    fn holding(&self) -> bool {
        self.lock.holding()
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
    pub fn sleep(&mut self) {
        self.lock
            .lock
            .waitchannel
            .sleep(self, &kernel_builder().current_proc().expect("No current proc"));
    }

    pub fn wakeup(&self) {
        self.lock.lock.waitchannel.wakeup();
    }
}

impl<T> Waitable for SleepablelockGuard<'_, T> {
    fn reacquire_spinlock_after<F, U>(&mut self, f: F) -> U
    where
        F: FnOnce() -> U,
    {
        self.lock.lock.release();
        let result = f();
        self.lock.lock.acquire();
        result
    }
}
