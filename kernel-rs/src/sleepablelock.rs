//! Sleepable locks
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

use crate::spinlock::RawSpinlock;
use crate::{
    kernel::kernel,
    proc::{WaitChannel, Waitable},
};

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

    pub fn lock(&self) -> SleepablelockGuard<'_, T> {
        self.lock.acquire();

        SleepablelockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.data.get()) }
    }
}

impl<T: Unpin> Sleepablelock<T> {
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    /// Returns a mutable pointer to the inner data.
    /// The returned pointer is valid until this lock is moved or dropped.
    /// The caller must ensure that accessing the pointer does not incur race.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a mutable reference to the inner data.
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
}

impl<T> SleepablelockGuard<'_, T> {
    pub fn sleep(&mut self) {
        self.lock
            .waitchannel
            .sleep(self, &kernel().current_proc().expect("No current proc"));
    }

    pub fn wakeup(&self) {
        self.lock.waitchannel.wakeup();
    }

    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        // Safe since for `T: !Unpin`, we only provide pinned references and don't move `T`.
        unsafe { Pin::new_unchecked(&mut *self.lock.data.get()) }
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

// We can mutably dereference the guard only when `T: Unpin`.
// If `T: !Unpin`, use `SleeplockGuard::get_pin()` instead.
impl<T: Unpin> DerefMut for SleepablelockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
