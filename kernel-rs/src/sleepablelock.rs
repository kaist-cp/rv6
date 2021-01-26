//! Sleepable locks
use crate::proc::{WaitChannel, Waitable};
use crate::spinlock::RawSpinlock;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

pub struct SleepablelockGuard<'s, T> {
    lock: Pin<&'s Sleepablelock<T>>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleepablelockGuard<'s, T> {}

/// Sleepable locks
#[pin_project]
pub struct Sleepablelock<T> {
    lock: RawSpinlock,
    /// WaitChannel saying spinlock is released.
    #[pin]
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
            lock: unsafe {
                // Safe since we maintain the `Pin` as long as the
                // `SleepablelockGuard` is alive.
                Pin::new_unchecked(self)
            },
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
    pub fn sleep(&mut self) {
        let lock = self.lock.project_ref();
        lock.waitchannel.sleep(self);
    }

    pub fn wakeup(&self) {
        self.lock.waitchannel.wakeup();
    }
}

impl<T> Waitable for SleepablelockGuard<'_, T> {
    unsafe fn raw_release(&mut self) {
        self.lock.lock.release();
    }
    unsafe fn raw_acquire(&mut self) {
        self.lock.lock.acquire();
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
