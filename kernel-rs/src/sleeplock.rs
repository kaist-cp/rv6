use crate::libc;
use crate::proc::{myproc, sleep, wakeup};
use crate::spinlock::{acquire, initlock, release, Spinlock};

#[derive(Copy, Clone)]
pub struct Sleeplock {
    pub locked: u32,
    pub lk: Spinlock,
    pub name: *mut libc::c_char,
    pub pid: i32,
}

impl Sleeplock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: 0,
            lk: Spinlock::zeroed(),
            name: 0 as *const libc::c_char as *mut libc::c_char,
            pid: 0,
        }
    }

    /// Sleeping locks
    pub unsafe fn new(name: *mut libc::c_char) -> Self {
        let mut lk = Self::zeroed();

        initlock(
            &mut lk.lk,
            b"sleep lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
        lk.name = name;
        lk.locked = 0 as u32;
        lk.pid = 0 as i32;

        lk
    }
}

pub unsafe fn initsleeplock(mut lk: *mut Sleeplock, mut name: *mut libc::c_char) {
    initlock(
        &mut (*lk).lk,
        b"sleep lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    (*lk).name = name;
    (*lk).locked = 0 as u32;
    (*lk).pid = 0 as i32;
}

pub unsafe fn acquiresleep(mut lk: *mut Sleeplock) {
    acquire(&mut (*lk).lk);
    while (*lk).locked != 0 {
        sleep(lk as *mut libc::c_void, &mut (*lk).lk);
    }
    (*lk).locked = 1 as u32;
    (*lk).pid = (*myproc()).pid;
    release(&mut (*lk).lk);
}

pub unsafe fn releasesleep(mut lk: *mut Sleeplock) {
    acquire(&mut (*lk).lk);
    (*lk).locked = 0 as u32;
    (*lk).pid = 0 as i32;
    wakeup(lk as *mut libc::c_void);
    release(&mut (*lk).lk);
}

pub unsafe fn holdingsleep(mut lk: *mut Sleeplock) -> i32 {
    let mut r: i32 = 0;
    acquire(&mut (*lk).lk);
    r = ((*lk).locked != 0 && (*lk).pid == (*myproc()).pid) as i32;
    release(&mut (*lk).lk);
    r
}
