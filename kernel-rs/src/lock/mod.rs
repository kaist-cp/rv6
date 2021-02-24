//! The lock module.
//!
//! Contains types for locks and lock guards that provide mutual exclusion,
//! and also includes traits that express their behaviors.

use core::pin::Pin;

mod rawspinlock;
mod sleepablelock;
mod sleeplock;
mod spinlock;
mod spinlock_protected;

pub use rawspinlock::*; //TODO: only use push_off/pop_off
pub use sleepablelock::{Sleepablelock, SleepablelockGuard};
pub use sleeplock::{Sleeplock, SleeplockGuard};
pub use spinlock::{Spinlock, SpinlockGuard};
pub use spinlock_protected::{SpinlockProtected, SpinlockProtectedGuard};

/// Represents lock guards that can be slept in a `WaitChannel`.
pub trait Waitable {
    /// Releases the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    /// Also, do not access `self` until re-acquiring the lock with `raw_acquire()`.
    unsafe fn raw_release(&mut self);

    /// Acquires the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    unsafe fn raw_acquire(&mut self);
}

/// Locks that own its own `RawLock` and `data: T`.
pub trait OwnedLock<T> {
    type Guard<'s>;

    /// Acquires the lock and returns the lock guard.
    fn lock(&self) -> Self::Guard<'_>;

    /// Returns a mutable reference to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    fn get_mut_raw(&self) -> *mut T;

    /// Returns a pinned mutable reference to the inner data.
    /// If `T: Unpin`, you can use the pin as a mutable reference or convert it into one by `Pin::get_mut()`.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.get_mut_raw()) }
    }

    /// Unlock the lock.
    ///
    /// # Safety
    ///
    /// Use this only when we acquired the lock but did `mem::forget()` to the guard.
    unsafe fn unlock(&self);

    /// Check whether this cpu is holding the lock.
    fn holding(&self) -> bool;
}

/// Locks that own its own `RawLock` and `data: T`, where `T: Unpin`.
pub trait UnpinLock<T: Unpin>: OwnedLock<T> {
    /// Consumes the lock and returns the inner data.
    fn into_inner(self) -> T;

    /// Returns a mutable reference to the inner data.
    fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.get_mut_raw() }
    }
}
