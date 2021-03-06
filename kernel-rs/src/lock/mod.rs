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

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

mod sleepablelock;
mod sleeplock;
mod spinlock;
mod spinlock_protected;

pub use sleepablelock::{Sleepablelock, SleepablelockGuard};
pub use sleeplock::{Sleeplock, SleeplockGuard};
pub use spinlock::{pop_off, push_off, EmptySpinlock, Spinlock, SpinlockGuard};
pub use spinlock_protected::{SpinlockProtected, SpinlockProtectedGuard};

pub trait RawLock {
    /// Acquires the lock.
    fn acquire(&self);
    /// Releases the lock.
    fn release(&self);
    /// Check whether this cpu is holding the lock.
    fn holding(&self) -> bool;
}

pub struct Lock<R: RawLock, T> {
    lock: R,
    data: UnsafeCell<T>,
}

unsafe impl<R: RawLock, T: Send> Sync for Lock<R, T> {}

pub struct Guard<'s, R: RawLock, T> {
    lock: &'s Lock<R, T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, R: RawLock, T: Sync> Sync for Guard<'s, R, T> {}

impl<R: RawLock, T> Lock<R, T> {
    /// Acquires the lock and returns the lock guard.
    pub fn lock(&self) -> Guard<'_, R, T> {
        self.lock.acquire();

        Guard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// Returns a mutable reference to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.get_mut_raw()) }
    }

    /// Returns a mutable reference to the inner data.
    pub fn get_mut(&mut self) -> &mut T
    where
        T: Unpin,
    {
        // Safe since we have a mutable reference of the lock.
        unsafe { &mut *self.get_mut_raw() }
    }

    /// Consumes the lock and returns the inner data.
    pub fn into_inner(self) -> T
    where
        T: Unpin,
    {
        self.data.into_inner()
    }

    // TODO: Add lock_and_forget()?

    /// Unlock the lock.
    ///
    /// # Safety
    ///
    /// Use this only when we acquired the lock but did `mem::forget()` to the guard.
    pub unsafe fn unlock(&self) {
        self.lock.release();
    }

    /// Check whether this cpu is holding the lock.
    pub fn holding(&self) -> bool {
        self.lock.holding()
    }
}

impl<R: RawLock, T> Guard<'_, R, T> {
    pub fn reacquire_after<F, U>(&mut self, f: F) -> U
    where
        F: FnOnce() -> U,
    {
        self.lock.lock.release();
        let result = f();
        self.lock.lock.acquire();
        result
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.lock.data.get()) }
    }

    /// Releases the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    /// Also, do not access `self` until re-acquiring the lock with `raw_acquire()`.
    pub unsafe fn raw_release(&mut self) {
        self.lock.lock.release();
    }

    /// Acquires the inner `RawSpinlock`.
    ///
    /// # Safety
    ///
    /// `raw_release()` and `raw_acquire` must always be used as a pair.
    /// Use these only for temporarily releasing (and then acquiring) the lock.
    pub unsafe fn raw_acquire(&mut self) {
        self.lock.lock.acquire();
    }
}

impl<R: RawLock, T> Drop for Guard<'_, R, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<R: RawLock, T> Deref for Guard<'_, R, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

// We can mutably dereference the guard only when `T: Unpin`.
// If `T: !Unpin`, use `Guard::get_pin_mut()` instead.
impl<R: RawLock, T: Unpin> DerefMut for Guard<'_, R, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}