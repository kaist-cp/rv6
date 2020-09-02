//! Sleeping locks
use crate::libc;
use crate::proc::{myproc, sleep, wakeup};
use crate::spinlock::RawSpinlock;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicI32, Ordering};

struct SleeplockInfo {
    /// For debugging:  

    /// Name of lock.
    name: &'static str,

    /// If the lock is held, contains `pid`.
    /// Otherwise, contains -1.
    /// Process holding lock
    pid: AtomicI32,
}

pub struct SleepLockGuard<'s, T> {
    lock: &'s SleeplockWIP<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SleepLockGuard<'s, T> {}

/// Long-term locks for processes
pub struct SleeplockWIP<T> {
    /// spinlock protecting this sleep lock
    rawlock: RawSpinlock,
    data: UnsafeCell<T>,
    info: SleeplockInfo,
}

unsafe impl<T: Send> Sync for SleeplockWIP<T> {}

impl<T> SleeplockWIP<T> {
    pub fn initlock(&mut self, name: &'static str) {
        (*self).rawlock.initlock("sleep lock");
        (*self).info = SleeplockInfo {
            name,
            pid: AtomicI32::new(-1),
        };
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub unsafe fn lock(&mut self) -> SleepLockGuard<'_, T> {
        self.rawlock.acquire();
        while (*self).info.pid.load(Ordering::Acquire) != -1 {
            sleep(
                self as *mut SleeplockWIP<T> as *mut libc::CVoid,
                &mut (*self).rawlock,
            );
        }
        (*self).info.pid.store((*myproc()).pid, Ordering::Release);
        self.rawlock.release();
        SleepLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }
}

impl<T> SleepLockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
    }

    pub fn unlock(&mut self) {
        self.lock.rawlock.acquire();
        self.lock.info.pid.store(-1, Ordering::Release);
        unsafe {
            wakeup(self.raw() as *mut SleeplockWIP<T> as *mut libc::CVoid);
        }
        self.lock.rawlock.release();
    }
}

impl<T> Drop for SleepLockGuard<'_, T> {
    fn drop(&mut self) {
        self.unlock();
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

/// Long-term locks for processes
pub struct Sleeplock {
    /// Is the lock held?
    locked: u32,

    /// spinlock protecting this sleep lock
    lk: RawSpinlock,

    /// For debugging:  

    /// Name of lock.
    name: &'static str,

    /// Process holding lock
    pid: i32,
}

impl Sleeplock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: 0,
            lk: RawSpinlock::zeroed(),
            name: "",
            pid: 0,
        }
    }

    pub unsafe fn new(name: &'static str) -> Self {
        let mut lk = Self::zeroed();

        lk.lk.initlock("sleep lock");
        lk.name = name;
        lk.locked = 0;
        lk.pid = 0;

        lk
    }

    pub fn initlock(&mut self, name: &'static str) {
        (*self).lk.initlock("sleep lock");
        (*self).name = name;
        (*self).locked = 0;
        (*self).pid = 0;
    }

    pub unsafe fn acquire(&mut self) {
        (*self).lk.acquire();
        while (*self).locked != 0 {
            sleep(self as *mut Sleeplock as *mut libc::CVoid, &mut (*self).lk);
        }
        (*self).locked = 1;
        (*self).pid = (*myproc()).pid;
        (*self).lk.release();
    }

    pub unsafe fn release(&mut self) {
        (*self).lk.acquire();
        (*self).locked = 0;
        (*self).pid = 0;
        wakeup(self as *mut Sleeplock as *mut libc::CVoid);
        (*self).lk.release();
    }

    pub unsafe fn holding(&mut self) -> i32 {
        (*self).lk.acquire();
        let r: i32 = ((*self).locked != 0 && (*self).pid == (*myproc()).pid) as i32;
        (*self).lk.release();
        r
    }
}
