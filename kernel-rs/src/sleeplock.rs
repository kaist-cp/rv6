//! Sleeping locks
use crate::proc::myproc;
use crate::sleepablelock::Sleepablelock;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

/// Long-term locks for processes
pub struct Sleeplock {
    /// Process holding lock. `-1` means unlocked.
    locked: Sleepablelock<i32>,

    /// Name of lock for debugging.
    name: &'static str,
}

impl Sleeplock {
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
        *guard = unsafe { (*myproc()).pid };
    }

    pub fn release(&self) {
        let mut guard = self.locked.lock();
        *guard = -1;
        guard.wakeup();
    }

    pub fn holding(&self) -> bool {
        let guard = self.locked.lock();
        *guard == unsafe { (*myproc()).pid }
    }
}

pub struct SleepLockGuard<'s, T> {
    lock: &'s SleeplockWIP<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleepLockGuard<'s, T> {}

pub struct SleeplockWIP<T> {
    lock: Sleeplock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SleeplockWIP<T> {}

impl<T> SleeplockWIP<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: Sleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub fn lock(&self) -> SleepLockGuard<'_, T> {
        self.lock.acquire();

        SleepLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `self` must not be shared by other threads. Use this function only in the middle of
    /// refactoring.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_mut_unchecked(&self) -> &mut T {
        &mut *self.data.get()
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
}

impl<T> SleepLockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
    }
}

impl<T> Drop for SleepLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SleepLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SleepLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
