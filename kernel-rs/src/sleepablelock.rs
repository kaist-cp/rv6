//! Sleepable locks
use crate::proc::WaitChannel;
use crate::spinlock::RawSpinlock;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

pub struct SleepablelockGuard<'s, T> {
    lock: &'s Sleepablelock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleepablelockGuard<'s, T> {}

/// Sleepable locks
pub struct Sleepablelock<T> {
    lock: RawSpinlock,
    /// WaitChannel saying spinlock is released.
    waitchannel: WaitChannel,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Sleepablelock<T> {}

impl<T> Sleepablelock<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            waitchannel: WaitChannel::new(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub fn lock(&self) -> SleepablelockGuard<'_, T> {
        self.lock.acquire();

        SleepablelockGuard {
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

impl<T> SleepablelockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
    }

    pub fn sleep(&mut self) {
        unsafe {
            self.lock
                .waitchannel
                .sleep_raw(&self.lock.lock as *const _ as *mut RawSpinlock);
        }
    }

    pub fn wakeup(&self) {
        self.lock.waitchannel.wakeup();
    }
}

impl<T> Drop for SleepablelockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SleepablelockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SleepablelockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
