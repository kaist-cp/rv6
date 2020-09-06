use crate::{
    bio::bget, fs::BSIZE, proc::WaitChannel, sleeplock::Sleeplock, virtio_disk::virtio_disk_rw,
};

use core::ptr;

pub struct Buf {
    pub dev: u32,
    pub blockno: u32,
    pub lock: Sleeplock,
    pub refcnt: u32,
    /// WaitChannel saying virtio_disk request is done.
    pub vdisk_request_waitchannel: WaitChannel,

    /// LRU cache list.
    pub prev: *mut Buf,
    pub next: *mut Buf,

    pub inner: BufInner,
}

impl Buf {
    pub const fn zeroed() -> Self {
        Self {
            dev: 0,
            blockno: 0,
            lock: Sleeplock::zeroed(),
            refcnt: 0,
            vdisk_request_waitchannel: WaitChannel::new(),

            prev: ptr::null_mut(),
            next: ptr::null_mut(),

            inner: BufInner::zeroed(),
        }
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        if (*self).lock.holding() == 0 {
            panic!("bwrite");
        }
        virtio_disk_rw(self, true);
    }

    /// Return a locked buf with the contents of the indicated block.
    pub unsafe fn read(dev: u32, blockno: u32) -> *mut Self {
        let b: *mut Self = bget(dev, blockno);
        if !(*b).inner.valid {
            virtio_disk_rw(b, false);
            (*b).inner.valid = true
        }
        b
    }
}

pub struct BufInner {
    /// Has data been read from disk?
    pub valid: bool,

    /// Does disk "own" buf?
    pub disk: bool,
    pub data: [u8; BSIZE],
}

impl BufInner {
    pub const fn zeroed() -> Self {
        Self {
            valid: false,
            disk: false,
            data: [0; BSIZE],
        }
    }
}
