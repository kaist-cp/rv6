//! Sleeping locks
use crate::libc;
use crate::proc::{myproc, sleep, wakeup};
use crate::spinlock::Spinlock;
use core::ptr;

/// Long-term locks for processes
pub struct Sleeplock {
    /// Is the lock held?
    locked: u32,

    /// spinlock protecting this sleep lock
    lk: Spinlock,

    /// For debugging:  

    /// Name of lock.
    name: *mut u8,

    /// Process holding lock
    pid: i32,
}

impl Sleeplock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: 0,
            lk: Spinlock::zeroed(),
            name: ptr::null_mut(),
            pid: 0,
        }
    }

    pub unsafe fn new(name: *mut u8) -> Self {
        let mut lk = Self::zeroed();

        lk.lk.initlock(b"sleep lock\x00" as *const u8 as *mut u8);
        lk.name = name;
        lk.locked = 0;
        lk.pid = 0;

        lk
    }

    pub fn initlock(&mut self, name: *mut u8) {
        (*self)
            .lk
            .initlock(b"sleep lock\x00" as *const u8 as *mut u8);
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
