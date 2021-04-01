use core::{cell::UnsafeCell, marker::PhantomData, pin::Pin, ptr};

use super::{spinlock::RawSpinlock, RawLock, Spinlock, SpinlockGuard, Waitable};

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

pub struct SpinlockProtectedGuard<'s> {
    lock: &'s RawSpinlock,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s> Sync for SpinlockProtectedGuard<'s> {}

impl<T, U> SpinlockProtected<T, &'static Spinlock<U>> {
    pub const fn new(lock: &'static Spinlock<U>, data: T) -> Self {
        Self {
            lock,
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockProtectedGuard<'_> {
        self.lock.lock.acquire();

        SpinlockProtectedGuard {
            lock: &self.lock.lock,
            _marker: PhantomData,
        }
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
    /// `guard: SpinlockProtectedGuard` was obtained from a `SpinlockProtected`
    /// that borrows the same `Spinlock` with this `SpinlockProtected`.
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
        guard: &'b mut SpinlockProtectedGuard<'_>,
    ) -> Pin<&'b mut T> {
        assert!(ptr::eq(&self.lock.lock, guard.lock));
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<T: Unpin, U> SpinlockProtected<T, &'static Spinlock<U>> {
    /// Returns a mutable reference to the inner data.
    /// See `SpinlockProtected::get_mut()` for details.
    pub fn get_mut<'a: 'b, 'b>(&'a self, guard: &'b mut SpinlockProtectedGuard<'_>) -> &'b mut T {
        self.get_pin_mut(guard).get_mut()
    }
}

impl SpinlockProtectedGuard<'_> {
    /// Converts `self` into a guard of the given `lock: Spinlock`.
    /// Panics if `self`'s corresponding `SpinlockProtected` was not obtained from the given `lock: Spinlock`.
    pub fn into_spinlock_guard<T>(self, lock: &Spinlock<T>) -> SpinlockGuard<'_, T> {
        assert!(ptr::eq(self.lock, &lock.lock));
        SpinlockGuard {
            lock,
            _marker: PhantomData,
        }
    }
}

impl Waitable for SpinlockProtectedGuard<'_> {
    fn reacquire_after<F, U>(&mut self, f: F) -> U
    where
        F: FnOnce() -> U,
    {
        self.lock.release();
        let result = f();
        self.lock.acquire();
        result
    }
}

impl Drop for SpinlockProtectedGuard<'_> {
    fn drop(&mut self) {
        self.lock.release();
    }
}
