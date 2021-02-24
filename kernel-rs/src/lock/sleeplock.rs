//! Sleeping locks
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

use super::{OwnedLock, Sleepablelock, UnpinLock};
use crate::kernel::kernel_builder;

/// Long-term locks for processes
struct RawSleeplock {
    /// Process holding lock. `-1` means unlocked.
    locked: Sleepablelock<i32>,

    /// Name of lock for debugging.
    name: &'static str,
}

impl RawSleeplock {
    pub const fn new(name: &'static str) -> Self {
        Self {
            locked: Sleepablelock::new("sleep lock", -1),
            name,
        }
    }

    pub fn acquire(&self) {
        let mut guard = self.locked.lock();
        while *guard != -1 {
            guard.sleep();
        }
        *guard = kernel_builder()
            .current_proc()
            .expect("No current proc")
            .pid();
    }

    pub fn release(&self) {
        let mut guard = self.locked.lock();
        *guard = -1;
        guard.wakeup();
    }

    pub fn holding(&self) -> bool {
        let guard = self.locked.lock();
        *guard
            == kernel_builder()
                .current_proc()
                .expect("No current proc")
                .pid()
    }
}

pub struct Sleeplock<T> {
    lock: RawSleeplock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Sleeplock<T> {}

pub struct SleeplockGuard<'s, T> {
    lock: &'s Sleeplock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleeplockGuard<'s, T> {}

impl<T> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    ///
    /// # Safety
    ///
    /// If `T: !Unpin`, `Sleeplock` or `SleeplockGuard` will only provide pinned mutable references
    /// of the inner data to the outside. However, it is still the caller's responsibility to
    /// make sure that the `Sleeplock` itself never gets moved.
    pub const unsafe fn new_unchecked(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: Unpin> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        // Safe since `T: Unpin`.
        unsafe { Self::new_unchecked(name, data) }
    }
}

impl<T: 'static> OwnedLock<T> for Sleeplock<T> {
    type Guard<'s> = SleeplockGuard<'s, T>;

    fn lock(&self) -> SleeplockGuard<'_, T> {
        self.lock.acquire();

        SleeplockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// Returns a mutable pointer to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
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

impl<T: 'static + Unpin> UnpinLock<T> for Sleeplock<T> {
    fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T> SleeplockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.lock.data.get()) }
    }
}

impl<T> Drop for SleeplockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SleeplockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

// We can mutably dereference the guard only when `T: Unpin`.
// If `T: !Unpin`, use `SleeplockGuard::get_pin_mut()` instead.
impl<T: Unpin> DerefMut for SleeplockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
