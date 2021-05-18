use core::{cell::UnsafeCell, marker::PhantomData, pin::Pin};

use super::{Guard, Lock, RawLock};

/// `RemoteLock<R, U, T>` is similar to `Lock<R, T>`, but uses a shared lock.
/// To access its inner data, the shared lock's guard is required.
/// We can make a single raw lock be shared by a `Lock` and multiple `RemoteLock`s.
/// See the [lock](`super`) module documentation for details.
///
/// # Note
///
/// To dereference the inner data, use `RemoteLock::get_pin_mut_unchecked`
/// or `RemoteLock::get_mut_unchecked`.
#[repr(transparent)]
pub struct RemoteLock<R: RawLock, U, T> {
    data: UnsafeCell<T>,
    _marker: PhantomData<*const Lock<R, U>>,
}

unsafe impl<'s, R: RawLock, U: Send, T: Send> Sync for RemoteLock<R, U, T> {}

impl<R: RawLock, U, T> RemoteLock<R, U, T> {
    /// Returns a `RemoteLock` that protects `data`.
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            _marker: PhantomData,
        }
    }

    /// Returns a raw pointer to the inner data.
    /// The returned pointer is valid until this `RemoteLock` is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a pinned mutable reference to the inner data.
    ///
    /// # Safety
    ///
    /// The provided `guard` must be from the `Lock` that this `RemoteLock` borrowed from.
    /// You may want to wrap this function with a safe function that uses branded types.
    pub unsafe fn get_pin_mut_unchecked<'t>(
        &'t self,
        _guard: &'t mut Guard<'_, R, U>,
    ) -> Pin<&'t mut T> {
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<'s, R: RawLock, U, T: Unpin> RemoteLock<R, U, T> {
    /// Returns a mutable reference to the inner data.
    ///
    /// # Safety
    ///
    /// The provided `guard` must be from the `Lock` that this `RemoteLock` borrowed from.
    /// You may want to wrap this function with a safe function that uses branded types.
    pub unsafe fn get_mut_unchecked<'t>(&'t self, guard: &'t mut Guard<'_, R, U>) -> &'t mut T {
        unsafe { self.get_pin_mut_unchecked(guard) }.get_mut()
    }
}
