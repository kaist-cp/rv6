//! The lock module.
//!
//! Contains types that provide mutual exclusion.
//!
//!
//! # Locks and [`Pin`]
//! Locks that own `!Unpin` data of type `T` should not give an `&mut T` of its data to the outside.
//! Similarly, we should not be able to mutably dereference a lock guard if the data `T` is `!Unpin`.
//! Otherwise, we could move the inner data, even when the lock itself is pinned.
//!
//! Therefore, locks in this module gives an `&mut T` to the outside only when `T: Unpin`.
//! Otherwise, it only gives a [`Pin<&mut T>`].
//! Similaraly, guards implement [DerefMut](`core::ops::DerefMut`) only when `T: Unpin`, and if `T: !Unpin`,
//! you should obtain a [`Pin<&mut T>`] from the guard and use it instead.
//!
//!
//! # SpinlockProtected
//! [`SpinlockProtected`] owns its data but does not have its own raw lock.
//! Instead, it borrows a raw lock from another [`Spinlock<()>`] and protects its data using it.
//! This is useful when multiple fragmented data must be protected by a single lock.
//! * e.g. By making multiple [`SpinlockProtected<T>`]s refer to a single [`Spinlock<()>`],
//!   you can make multiple data be protected by a single [`Spinlock<()>`], and hence,
//!   implement global locks.

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
pub use spinlock::{pop_off, push_off, Spinlock, SpinlockGuard};
pub use spinlock_protected::{SpinlockProtected, SpinlockProtectedGuard};

pub trait RawLock {
    /// Acquires the lock.
    fn acquire(&self);
    /// Releases the lock.
    fn release(&self);
    /// Check whether this cpu is holding the lock.
    fn holding(&self) -> bool;
}

pub trait Waitable {
    /// Temporarily releases the lock and calls function `f`.
    /// After `f` returns, reacquires the lock and returns the result of the function call.
    fn reacquire_inner_spinlock<F, U>(&mut self, f: F) -> U
    where
        F: FnOnce() -> U;
}

/// Locks that provide mutual exclusion and has its own `RawLock`.
pub struct Lock<R: RawLock, T> {
    lock: R,
    data: UnsafeCell<T>,
}

unsafe impl<R: RawLock, T: Send> Sync for Lock<R, T> {}

/// Guards that guarantee exclusive mutable access to the lock's inner data.
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

    /// Unlock the lock.
    ///
    /// # Safety
    ///
    /// Use this only when we acquired the lock but did `mem::forget()` to the guard.
    pub unsafe fn unlock(&self) {
        self.lock.release();
    }
}

impl<R: RawLock, T> Guard<'_, R, T> {
    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.lock.data.get()) }
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
        self.get_pin_mut().get_mut()
    }
}
