//! The lock module.
//! Contains types that provide mutual exclusion.
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
//! # RemoteLock
//! A `RemoteLock` owns its data but does not have its own raw lock.
//! Instead, it borrows another [`Lock`] (such as [`Spinlock`], [`Sleepablelock`], or [`Sleeplock`]) and protects its data using it.
//! That is, a [`Lock`] protects its own data *and* all other connected `RemoteLock`s' data.
//!
//! This is useful in several cases.
//! * When multiple fragmented data must be protected by a single lock.
//!   * e.g. By making multiple `RemoteLock`s borrow a single [`Spinlock`],
//!     you can make multiple data be protected by a single [`Spinlock`], and hence,
//!     implement global locks. In this case, you may want to use an [`Spinlock<()>`]
//!     if the [`Spinlock`] doesn't need to hold data.
//! * When you want a lifetime-less smart pointer (such as [`Ref`](crate::util::rc_cell::Ref) or `std::rc::Rc`)
//!   that points to the *inside* of a lock protected data.
//!   * e.g. Suppose a [`Lock`] holds a [`RcCell`](crate::util::rc_cell::RcCell). Suppose you want to provide a
//!     [`Ref`](crate::util::rc_cell::Ref) that borrows this [`RcCell`](crate::util::rc_cell::RcCell) to the outside, but still want
//!     accesses to the [`RcCell`](crate::util::rc_cell::RcCell)'s inner data to be synchronized.
//!     Then, instead of providing a [`Ref`](crate::util::rc_cell::Ref), you should provide a [`Ref`](crate::util::rc_cell::Ref) wrapped by a `RemoteLock`.
//!     to the outside.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

mod remotelock;
mod sleepablelock;
mod sleeplock;
mod spinlock;

pub use remotelock::RemoteLock;
pub use sleepablelock::{Sleepablelock, SleepablelockGuard};
pub use sleeplock::{Sleeplock, SleeplockGuard};
pub use spinlock::{RawSpinlock, Spinlock, SpinlockGuard};

pub trait RawLock {
    /// Acquires the lock.
    fn acquire(&self);
    /// Releases the lock.
    fn release(&self);
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

    /// Returns a raw pointer to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        // SAFETY: for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.get_mut_raw()) }
    }

    /// Returns a mutable reference to the inner data.
    pub fn get_mut(&mut self) -> &mut T
    where
        T: Unpin,
    {
        // SAFETY: we have a mutable reference of the lock.
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
    /// Temporarily releases the lock and calls function `f`.
    /// After `f` returns, reacquires the lock and returns the result of the function call.
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
        // SAFETY: for `T: !Unpin`, we only provide pinned references and don't move `T`.
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
