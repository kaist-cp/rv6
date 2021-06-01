//! Sleeping locks
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use super::Sleepablelock;
use crate::proc::KernelCtx;

/// Long-term locks for processes
pub struct RawSleeplock {
    /// Process holding lock. `-1` means unlocked.
    inner: Sleepablelock<i32>,
}

/// Locks that sleep instead of busy wait.
pub struct Sleeplock<T> {
    lock: RawSleeplock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Sleeplock<T> {}

/// Guards of `Sleeplock<T>`.
pub struct SleeplockGuard<'s, T> {
    lock: &'s Sleeplock<T>,
    _marker: PhantomData<*const ()>,
}

unsafe impl<'s, T: Sync> Sync for SleeplockGuard<'s, T> {}

impl RawSleeplock {
    const fn new(name: &'static str) -> Self {
        Self {
            inner: Sleepablelock::new(name, -1),
        }
    }

    fn acquire(&self, ctx: &KernelCtx<'_, '_>) {
        let mut guard = self.inner.lock();
        while *guard != -1 {
            guard.sleep(ctx);
        }
        *guard = ctx.proc().pid();
    }

    fn release(&self, ctx: &KernelCtx<'_, '_>) {
        let mut guard = self.inner.lock();
        *guard = -1;
        guard.wakeup(ctx.kernel());
    }
}

impl<T> Sleeplock<T> {
    /// Returns a new `Sleeplock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSleeplock::new(name),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquires the lock and returns the lock guard.
    pub fn lock(&self, ctx: &KernelCtx<'_, '_>) -> SleeplockGuard<'_, T> {
        self.lock.acquire(ctx);

        SleeplockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// Returns a raw pointer to the inner data.
    pub fn get_mut_raw(&self) -> *mut T {
        self.data.get()
    }

    /// Returns a mutable reference to the inner data.
    pub fn get_mut(&mut self) -> &mut T
    where
        T: Unpin,
    {
        // SAFETY: we have a mutable reference of the lock.
        unsafe { &mut *self.get_mut_raw() }
    }

    /// Unlock the lock.
    ///
    /// # Safety
    ///
    /// Use this only when we acquired the lock but did `mem::forget()` to the guard.
    pub unsafe fn unlock(&self, ctx: &KernelCtx<'_, '_>) {
        self.lock.release(ctx);
    }
}

impl<T> SleeplockGuard<'_, T> {
    pub fn free(self, ctx: &KernelCtx<'_, '_>) {
        self.lock.lock.release(ctx);
        core::mem::forget(self);
    }
}

impl<T> Drop for SleeplockGuard<'_, T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("SleeplockGuard must never drop.");
    }
}

impl<T> Deref for SleeplockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

// We can mutably dereference the guard only when `T: Unpin`.
// If `T: !Unpin`, use `Guard::get_pin_mut()` instead.
impl<T: Unpin> DerefMut for SleeplockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}
