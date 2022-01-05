//! The lock module.
//! Contains types that provide mutual exclusion.

mod sleepablelock;
mod sleeplock;
mod spinlock;

pub use sleepablelock::{
    new_sleepable_lock, sleep_guard, wakeup_guard, SleepableLock, SleepableLockGuard,
};
pub use sleeplock::{SleepLock, SleepLockGuard};
pub use spinlock::{new_spin_lock, RawSpinLock, SpinLock, SpinLockGuard};
