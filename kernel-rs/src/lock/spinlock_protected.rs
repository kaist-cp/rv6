use core::{cell::UnsafeCell, marker::PhantomData, pin::Pin, ptr};

use super::{RawSpinlock, Waitable};

/// Similar to `Spinlock<T>`, but instead of internally owning a `RawSpinlock`,
/// this stores a `'static` reference to an external `RawSpinlock` that was provided by the caller.
/// By making multiple `SpinlockProtected<T>`'s refer to a single `RawSpinlock`,
/// you can make multiple data be protected by a single `RawSpinlock`, and hence,
/// implement global locks.
/// To dereference the inner data, you must use `SpinlockProtected<T>::get_mut`, instead of
/// trying to dereference the `SpinlockProtectedGuard`.
pub struct SpinlockProtected<T> {
    lock: &'static RawSpinlock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinlockProtected<T> {}

pub struct SpinlockProtectedGuard<'s> {
    lock: &'s RawSpinlock,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s> Sync for SpinlockProtectedGuard<'s> {}

impl<T> SpinlockProtected<T> {
    pub const fn new(raw_lock: &'static RawSpinlock, data: T) -> Self {
        Self {
            lock: raw_lock,
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockProtectedGuard<'_> {
        self.lock.acquire();

        SpinlockProtectedGuard {
            lock: self.lock,
            _marker: PhantomData,
        }
    }

    /// Check whether this cpu is holding the lock.
    pub fn holding(&self) -> bool {
        self.lock.holding()
    }

    /// Returns a pinned mutable reference to the inner data.
    /// See `SpinlockProtected::get_mut()` for details.
    pub fn get_pin_mut<'a: 'b, 'b>(
        &'a self,
        guard: &'b mut SpinlockProtectedGuard<'_>,
    ) -> Pin<&'b mut T> {
        assert!(ptr::eq(self.lock, guard.lock));
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<T: Unpin> SpinlockProtected<T> {
    /// Returns a mutable reference to the inner data, provided that the given
    /// `guard: SpinlockProtectedGuard` was obtained from a `SpinlockProtected`
    /// that refers to the same `RawSpinlock` with this `SpinlockProtected`.
    ///
    /// # Note
    ///
    /// In order to prevent references from leaking, the returned reference
    /// cannot outlive the given `guard`.
    ///
    /// This method adds some small runtime cost, since we need to check that the given
    /// `SpinlockProtectedGuard` was truely originated from a `SpinlockProtected`
    /// that refers to the same `RawSpinlock`.
    /// TODO(https://github.com/kaist-cp/rv6/issues/375)
    /// This runtime cost can be removed by using a trait, such as `pub trait SpinlockID {}`.
    pub fn get_mut<'a: 'b, 'b>(&'a self, guard: &'b mut SpinlockProtectedGuard<'_>) -> &'b mut T {
        assert!(ptr::eq(self.lock, guard.lock));
        unsafe { &mut *self.data.get() }
    }
}

impl Waitable for SpinlockProtectedGuard<'_> {
    unsafe fn raw_release(&mut self) {
        self.lock.release();
    }

    unsafe fn raw_acquire(&mut self) {
        self.lock.acquire();
    }
}

impl Drop for SpinlockProtectedGuard<'_> {
    fn drop(&mut self) {
        self.lock.release();
    }
}
