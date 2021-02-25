//! The lock module.
//!
//! Contains types for locks and lock guards that provide mutual exclusion,
//! and also includes traits that express their behaviors.
//!
//! # Locks and [`Pin`]
//! Locks that own `!Unpin` data of type `T` should not give an `&mut T` of its data to the outside.
//! Similarly, we should not be able to mutably dereference a lock guard if the data `T` is `!Unpin`.
//! Otherwise, we could move the inner data, even when the lock itself is pinned.
//!
//! Therefore, locks in this module gives an `&mut T` to the outside only when `T: Unpin`.
//! Otherwise, it only gives a [`Pin<&mut T>`].
//! Similaraly, guards implement `DerefMut` only when `T: Unpin`, and if `T: !Unpin`,
//! you should obtain a [`Pin<&mut T>`] from the guard and use it instead.
//!
//! # SpinlockProtected
//! TODO

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

/// Lock guards that can be slept in a `WaitChannel`.
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

/// Locks that own a raw lock.
pub trait Lock {
    type Data;
    type Guard<'s>;

    /// Acquires the lock and returns the lock guard.
    fn lock(&self) -> Self::Guard<'_>;

    /// Returns a mutable reference to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    fn get_mut_raw(&self) -> *mut Self::Data;

    /// Returns a pinned mutable reference to the inner data.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut Self::Data> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.get_mut_raw()) }
    }

    /// Returns a mutable reference to the inner data.
    fn get_mut(&mut self) -> &mut Self::Data
    where
        Self::Data: Unpin,
    {
        // Safe since we have a mutable reference of the lock.
        unsafe { &mut *self.get_mut_raw() }
    }

    /// Consumes the lock and returns the inner data.
    fn into_inner(self) -> Self::Data
    where
        Self::Data: Unpin;

    /// Unlock the lock.
    ///
    /// # Safety
    ///
    /// Use this only when we acquired the lock but did `mem::forget()` to the guard.
    unsafe fn unlock(&self);

    /// Check whether this cpu is holding the lock.
    fn holding(&self) -> bool;
}
