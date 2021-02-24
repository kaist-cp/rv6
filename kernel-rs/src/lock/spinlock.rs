use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

use super::{OwnedLock, RawSpinlock, UnpinLock, Waitable};

pub struct Spinlock<T> {
    lock: RawSpinlock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}

pub struct SpinlockGuard<'s, T> {
    lock: &'s Spinlock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SpinlockGuard<'s, T> {}

impl<T> Spinlock<T> {
    /// Returns a new `Spinlock` with name `name` and data `data`.
    ///
    /// # Safety
    ///
    /// If `T: !Unpin`, `Spinlock` or `SpinlockGuard` will only provide pinned mutable references
    /// of the inner data to the outside. However, it is still the caller's responsibility to
    /// make sure that the `Spinlock` itself never gets moved.
    pub const unsafe fn new_unchecked(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: Unpin> Spinlock<T> {
    /// Returns a new `Spinlock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        // Safe since `T: Unpin`.
        unsafe { Self::new_unchecked(name, data) }
    }
}

impl<T: 'static> OwnedLock<T> for Spinlock<T> {
    type Guard<'s> = SpinlockGuard<'s, T>;

    fn lock(&self) -> SpinlockGuard<'_, T> {
        self.lock.acquire();

        SpinlockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    unsafe fn unlock(&self) {
        self.lock.release();
    }

    fn holding(&self) -> bool {
        self.lock.holding()
    }
}

impl<T: 'static + Unpin> UnpinLock<T> for Spinlock<T> {
    fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T> SpinlockGuard<'_, T> {
    pub fn reacquire_after<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
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
}

impl<T> Waitable for SpinlockGuard<'_, T> {
    unsafe fn raw_release(&mut self) {
        self.lock.lock.release();
    }

    unsafe fn raw_acquire(&mut self) {
        self.lock.lock.acquire();
    }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SpinlockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

// We can mutably dereference the guard only when `T: Unpin`.
// If `T: !Unpin`, use `SpinlockGuard::get_pin_mut()` instead.
impl<T: Unpin> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
