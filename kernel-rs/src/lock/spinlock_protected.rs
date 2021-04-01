use core::{cell::UnsafeCell, pin::Pin, ptr};

use super::{Spinlock, SpinlockGuard};

/// `SpinlockProtected<T, &Spinlock<U>>`.
/// Similar to `Spinlock<T>`, but uses a shared raw lock.
/// At creation, a `SpinlockProtected<T>` borrows a raw lock from a `Spinlock` and uses it to protect its data.
/// In this way, we can make a single raw lock be shared by a `Spinlock` and multiple `SpinlockProtected`s.
/// * See the [lock](`super`) module documentation for details.
///
/// # Note
///
/// To dereference the inner data, you must use `SpinlockProtected<T>::get_mut`, instead of
/// trying to dereference the `SpinlockProtectedGuard`.
pub struct SpinlockProtected<T, P> {
    lock: P,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send, P> Sync for SpinlockProtected<T, P> {}

impl<T, U> SpinlockProtected<T, &'static Spinlock<U>> {
    /// Returns a `SpinlockProtected` that protects `data` using the given `lock`.
    pub const fn new(lock: &'static Spinlock<U>, data: T) -> Self {
        Self {
            lock,
            data: UnsafeCell::new(data),
        }
    }

    /// Acquires the lock and returns the `SpinlockGuard`.
    /// * To access `self`'s inner data, use `SpinlockProtected::get_pin_mut` with the returned guard.
    /// * To access the borrowed `Spinlock`'s data, just dereference the returned guard.
    pub fn lock(&self) -> SpinlockGuard<'_, U> {
        self.lock.lock()
    }

    /// Returns a reference to the `Spinlock` that `self` borrowed from.
    pub fn get_spinlock(&self) -> &'static Spinlock<U> {
        self.lock
    }

    /// Returns a raw pointer to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a pinned mutable reference to the inner data, provided that the given
    /// `guard` was obtained by `lock()`ing `self` or `self`'s corresponding `Spinlock`.
    /// Otherwise, panics.
    ///
    /// # Note
    ///
    /// In order to prevent references from leaking, the returned reference
    /// cannot outlive the given `guard`.
    ///
    /// This method adds some small runtime cost, since we need to check that the given
    /// `SpinlockProtectedGuard` was truely originated from a `SpinlockProtected`
    /// that borrows the same `Spinlock`.
    /// TODO(https://github.com/kaist-cp/rv6/issues/375)
    /// This runtime cost can be removed by using a trait, such as `pub trait SpinlockID {}`.
    pub fn get_pin_mut<'a: 'b, 'b>(
        &'a self,
        guard: &'b mut SpinlockGuard<'_, U>,
    ) -> Pin<&'b mut T> {
        assert!(ptr::eq(self.lock, guard.lock));
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<T: Unpin, U> SpinlockProtected<T, &'static Spinlock<U>> {
    /// Returns a mutable reference to the inner data.
    /// See `SpinlockProtected::get_mut()` for details.
    pub fn get_mut<'a: 'b, 'b>(&'a self, guard: &'b mut SpinlockGuard<'_, U>) -> &'b mut T {
        self.get_pin_mut(guard).get_mut()
    }
}
