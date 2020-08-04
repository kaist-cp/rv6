use crate::libc;
use crate::proc::{myproc, sleep, wakeup};
use crate::spinlock::Spinlock;
use core::ptr;

#[derive(Copy, Clone)]
pub struct Sleeplock {
    locked: u32,
    lk: Spinlock,
    name: *mut libc::c_char,
    pid: i32,
}

impl Sleeplock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: 0,
            lk: Spinlock::zeroed(),
            name: ptr::null_mut() as *const libc::c_char as *mut libc::c_char,
            pid: 0,
        }
    }

    /// Sleeping locks
    pub unsafe fn new(name: *mut libc::c_char) -> Self {
        let mut lk = Self::zeroed();

        lk.lk
            .initlock(b"sleep lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        lk.name = name;
        lk.locked = 0 as u32;
        lk.pid = 0 as i32;

        lk
    }

    pub fn initlock(&mut self, mut name: *mut libc::c_char) {
        (*self)
            .lk
            .initlock(b"sleep lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        (*self).name = name;
        (*self).locked = 0 as u32;
        (*self).pid = 0 as i32;
    }

    pub unsafe fn acquire(&mut self) {
        (*self).lk.acquire();
        while (*self).locked != 0 {
            sleep(self as *mut Sleeplock as *mut libc::c_void, &mut (*self).lk);
        }
        (*self).locked = 1 as u32;
        (*self).pid = (*myproc()).pid;
        (*self).lk.release();
    }

    pub unsafe fn release(&mut self) {
        (*self).lk.acquire();
        (*self).locked = 0 as u32;
        (*self).pid = 0 as i32;
        wakeup(self as *mut Sleeplock as *mut libc::c_void);
        (*self).lk.release();
    }

    pub unsafe fn holding(&mut self) -> i32 {
        let mut r: i32 = 0;
        (*self).lk.acquire();
        r = ((*self).locked != 0 && (*self).pid == (*myproc()).pid) as i32;
        (*self).lk.release();
        r
    }
}
