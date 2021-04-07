use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr,
};

use super::{
    sleepablelock::RawSleepablelock,
    sleeplock::RawSleeplock,
    spinlock::{RawSpinlock, Spinlock},
    Guard, Lock, RawLock,
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

/* Experimental */
// TODO: Add more documentation

/// A type that holds an invariant, unique lifetime called `'id`.
/// This lifetime is actually more like an identifier/token, rather than a lifetime.
type Id<'id> = PhantomData<Cell<&'id mut ()>>;

/// A branded lock.
/// The `'id` tag uniquely distinguishes `BrandedLock`s and its related `BrandedGuard`/`BrandedRemoteLock`.
pub struct BrandedLock<'id, R: RawLock, T> {
    _marker: Id<'id>,
    lock: Lock<R, T>,
}

pub type BrandedSleepablelock<'id, T> = BrandedLock<'id, RawSleepablelock, T>;
pub type BrandedSleeplock<'id, T> = BrandedLock<'id, RawSleeplock, T>;
pub type BrandedSpinlock<'id, T> = BrandedLock<'id, RawSpinlock, T>;

pub struct BrandedGuard<'id, 's, R: RawLock, T> {
    _marker: Id<'id>,
    guard: Guard<'s, R, T>,
}

pub type BrandedSleepablelockGuard<'id, 's, T> = BrandedGuard<'id, 's, RawSleepablelock, T>;
pub type BrandedSleeplockGuard<'id, 's, T> = BrandedGuard<'id, 's, RawSleeplock, T>;
pub type BrandedSpinlockGuard<'id, 's, T> = BrandedGuard<'id, 's, RawSpinlock, T>;

pub struct BrandedRemoteLock<'id, 's, R: RawLock, U, T> {
    lock: &'s BrandedLock<'id, R, U>,
    data: UnsafeCell<T>,
}

pub type BrandedRemoteSleepablelock<'id, 's, U, T> =
    BrandedRemoteLock<'id, 's, RawSleepablelock, U, T>;
pub type BrandedRemoteSleeplock<'id, 's, U, T> = BrandedRemoteLock<'id, 's, RawSleeplock, U, T>;
pub type BrandedRemoteSpinlock<'id, 's, U, T> = BrandedRemoteLock<'id, 's, RawSpinlock, U, T>;

/* impl BrandedLock */
impl<'id, T> BrandedSpinlock<'id, T> {
    /// Creates a new `BrandedSpinlock` that can be used within the given closure.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F: for<'new_id> FnOnce(BrandedSpinlock<'new_id, T>) -> V, V>(
        name: &'static str,
        data: T,
        f: F,
    ) -> V {
        f(Self {
            _marker: PhantomData,
            lock: Spinlock::new(name, data),
        })
    }
}

impl<'id, R: RawLock, T> BrandedLock<'id, R, T> {
    pub fn lock(&self) -> BrandedGuard<'id, '_, R, T> {
        BrandedGuard {
            _marker: PhantomData,
            guard: self.lock.lock(),
        }
    }
}

impl<'id, R: RawLock, T> Deref for BrandedLock<'id, R, T> {
    type Target = Lock<R, T>;

    fn deref(&self) -> &Self::Target {
        &self.lock
    }
}

impl<'id, R: RawLock, T> DerefMut for BrandedLock<'id, R, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lock
    }
}

/* impl BrandedGuard */
impl<'id, 's, R: RawLock, T> Deref for BrandedGuard<'id, 's, R, T> {
    type Target = Guard<'s, R, T>;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'id, 's, R: RawLock, T> DerefMut for BrandedGuard<'id, 's, R, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

/* impl BrandedRemoteLock */
impl<'id, 's, R: RawLock, U, T> BrandedRemoteLock<'id, 's, R, U, T> {
    pub fn new(lock: &'s BrandedLock<'id, R, U>, data: T) -> Self {
        Self {
            lock,
            data: UnsafeCell::new(data),
        }
    }

    pub fn get_pin_mut<'a>(&self, _guard: &'a mut BrandedGuard<'id, '_, R, U>) -> Pin<&'a mut T> {
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

fn test() {
    BrandedSpinlock::new("lock1", 10, |lock1| {
        BrandedSpinlock::new("lock2", 20, |lock2| {
            let mut guard1 = lock1.lock();
            let mut guard2 = lock2.lock();

            let remote_lock1 = BrandedRemoteSpinlock::new(&lock1, 100);
            let remote_lock2 = BrandedRemoteSpinlock::new(&lock2, 200);
            assert!(*remote_lock1.get_pin_mut(&mut guard1) == 100);
            assert!(*remote_lock2.get_pin_mut(&mut guard2) == 200);
            // assert!(*remote_lock1.get_mut(&mut guard2) == 100); // Error! Compile fails!
        });
    });
}
