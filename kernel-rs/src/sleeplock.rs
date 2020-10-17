//! Sleeping locks
use crate::proc::myproc;
use crate::sleepablelock::Sleepablelock;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

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

pub struct SleeplockGuard<'s, T> {
    lock: &'s Sleeplock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleeplockGuard<'s, T> {}

pub struct Sleeplock<T> {
    lock: RawSleeplock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Sleeplock<T> {}

impl<T> Sleeplock<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub fn lock(&self) -> SleeplockGuard<'_, T> {
        self.lock.acquire();

        SleeplockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    pub unsafe fn unlock(&self) {
        self.lock.release();
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

impl<T> SleeplockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
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

impl<T> DerefMut for SleeplockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
