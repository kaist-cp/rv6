//! Sleepable locks
use core::cell::UnsafeCell;

use super::{spinlock::RawSpinlock, Guard, Lock, RawLock};
use crate::{kernel::kernel_builder, proc::WaitChannel};

/// Mutual exclusion spin locks that can sleep.
pub struct RawSleepablelock {
    lock: RawSpinlock,
    /// WaitChannel saying spinlock is released.
    waitchannel: WaitChannel,
}

/// Similar to `Spinlock`, but guards of this lock can sleep.   
pub type Sleepablelock<T> = Lock<RawSleepablelock, T>;
pub type SleepablelockGuard<'s, T> = Guard<'s, RawSleepablelock, T>;

impl RawSleepablelock {
    /// Mutual exclusion spin locks.
    const fn new(name: &'static str) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            waitchannel: WaitChannel::new(),
        }
    }

    pub fn sleep<T>(&self, guard: &mut Guard<'_, Self, T>) {
        self.waitchannel
            .sleep(guard, &kernel_builder().current_proc().expect("No current proc"));
    }

    pub fn wakeup(&self) {
        self.waitchannel.wakeup();
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
        self.lock.lock.sleep(self);
    }

    pub fn wakeup(&self) {
        self.lock.lock.wakeup();
    }
}
