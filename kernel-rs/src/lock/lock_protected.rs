use core::{cell::UnsafeCell, pin::Pin, ptr};

use super::{
    sleepablelock::RawSleepablelock, sleeplock::RawSleeplock, spinlock::RawSpinlock, Guard, Lock,
    RawLock,
};

/// `RemoteLock<'s, R, U, T>`, such as `RemoteLock<'s, RawSpinlock, U, T>`.
/// Similar to `Lock<R, T>`, but uses a shared raw lock.
/// At creation, a `RemoteLock` borrows a raw lock from a `Lock` and uses it to protect its data.
/// In this way, we can make a single raw lock be shared by a `Lock` and multiple `RemoteLock`s.
/// * See the [lock](`super`) module documentation for details.
///
/// # Note
///
/// To dereference the inner data, you must use `RemoteLock::get_mut`.
pub struct RemoteLock<'s, R: RawLock, U, T> {
    lock: &'s Lock<R, U>,
    data: UnsafeCell<T>,
}

unsafe impl<'s, R: RawLock, U: Send, T: Send> Sync for RemoteLock<'s, R, U, T> {}

/// A `RemoteLock` that borrows a `Sleepablelock<U>`.
pub type RemoteSleepablelock<'s, U, T> = RemoteLock<'s, RawSleepablelock, U, T>;
/// A `RemoteLock` that borrows a `Sleeplock<U>`.
pub type RemoteSleeplock<'s, U, T> = RemoteLock<'s, RawSleeplock, U, T>;
/// A `RemoteLock` that borrows a `Spinlock<U>`.
pub type RemoteSpinlock<'s, U, T> = RemoteLock<'s, RawSpinlock, U, T>;

impl<'s, R: RawLock, U, T> RemoteLock<'s, R, U, T> {
    /// Returns a `RemoteLock` that protects `data` using the given `lock`.
    /// `lock` could be any [`Lock`], such as [Spinlock](super::Spinlock), [Sleepablelock](super::Sleepablelock), or [Sleeplock](super::Sleeplock).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let spinlock: Spinlock<usize> = Spinlock::new("spinlock", 10);
    /// let spinlock_protected: RemoteSpinlock<'_, usize, isize> = RemoteLock::new(&spinlock, -20);
    /// ```
    pub const fn new(lock: &'s Lock<R, U>, data: T) -> Self {
        Self {
            lock,
            data: UnsafeCell::new(data),
        }
    }

    /// Acquires the lock and returns the `Guard`.
    /// * To access `self`'s inner data, use `RemoteLock::get_pin_mut` with the returned guard.
    /// * To access the borrowed `Lock`'s data, just dereference the returned guard.
    pub fn lock(&self) -> Guard<'_, R, U> {
        self.lock.lock()
    }

    /// Returns a reference to the `Lock` that `self` borrowed from.
    pub fn get_lock(&self) -> &'s Lock<R, U> {
        self.lock
    }

    /// Returns a raw pointer to the inner data.
    /// The returned pointer is valid until this `RemoteLock` is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    /// Also, if `T: !Unpin`, the caller must not move the data using the pointer.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a pinned mutable reference to the inner data, provided that the given
    /// `guard` was obtained by `lock()`ing `self` or `self`'s corresponding `Lock`.
    /// Otherwise, panics.
    ///
    /// # Note
    ///
    /// In order to prevent references from leaking, the returned reference
    /// cannot outlive the given `guard`.
    ///
    /// This method adds some small runtime cost, since we need to check that the given
    /// `Guard` was truely obtained by `lock()`ing `self` or `self`'s corresponding `Lock`.
    /// TODO(https://github.com/kaist-cp/rv6/issues/375)
    /// This runtime cost can be removed by using a trait, such as `pub trait LockID {}`.
    pub fn get_pin_mut<'a: 'b, 'b>(&'a self, guard: &'b mut Guard<'_, R, U>) -> Pin<&'b mut T> {
        assert!(ptr::eq(self.lock, guard.lock));
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<'s, R: RawLock, U, T: Unpin> RemoteLock<'s, R, U, T> {
    /// Returns a mutable reference to the inner data.
    /// See `RemoteLock::get_pin_mut()` for details.
    pub fn get_mut<'a: 'b, 'b>(&'a self, guard: &'b mut Guard<'_, R, U>) -> &'b mut T {
        self.get_pin_mut(guard).get_mut()
    }
}
