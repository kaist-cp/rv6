use crate::{bio::BCACHE, fs::BSIZE, sleeplock::Sleeplock, virtio_disk::virtio_disk_rw};

use core::ptr;

pub struct Buf {
    /// Has data been read from disk?
    pub valid: bool,

    pub dev: u32,
    pub blockno: u32,
    pub lock: Sleeplock,
    pub refcnt: u32,

    /// LRU cache list.
    pub prev: *mut Buf,
    pub next: *mut Buf,

    pub bufinner: BufInner,
}

pub struct BufInner {
    /// Does disk "own" buf?
    pub disk: bool,
    pub data: [u8; BSIZE],
}

impl BufInner {
    pub const fn zeroed() -> Self {
        Self {
            disk: false,
            data: [0; BSIZE],
        }
    }
}

impl Buf {
    pub const fn zeroed() -> Self {
        Self {
            valid: false,
            dev: 0,
            blockno: 0,
            lock: Sleeplock::zeroed(),
            refcnt: 0,

            prev: ptr::null_mut(),
            next: ptr::null_mut(),

            bufinner: BufInner::zeroed(),
        }
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        if (*self).lock.holding() == 0 {
            panic!("bwrite");
        }
        virtio_disk_rw(self, true);
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    pub unsafe fn release(&mut self) {
        if (*self).lock.holding() == 0 {
            panic!("brelease");
        }
        (*self).lock.release();
        let mut bcache = BCACHE.lock();
        (*self).refcnt = (*self).refcnt.wrapping_sub(1);
        if (*self).refcnt == 0 {
            // No one is waiting for it.
            (*(*self).next).prev = (*self).prev;
            (*(*self).prev).next = (*self).next;
            (*self).next = bcache.head.next;
            (*self).prev = &mut bcache.head;
            (*bcache.head.next).prev = self;
            bcache.head.next = self
        }
    }

    pub unsafe fn pin(&mut self) {
        let bcache = BCACHE.lock();
        (*self).refcnt = (*self).refcnt.wrapping_add(1);
        drop(bcache);
    }

    pub unsafe fn unpin(&mut self) {
        let bcache = BCACHE.lock();
        (*self).refcnt = (*self).refcnt.wrapping_sub(1);
        drop(bcache);
    }
    /// Look through buffer cache for block on device dev.
    /// If not found, allocate a buffer.
    /// In either case, return locked buffer.
    unsafe fn get(dev: u32, blockno: u32) -> *mut Self {
        let mut bcache = BCACHE.lock();

        // Is the block already cached?
        let mut b: *mut Self = bcache.head.next;
        while b != &mut bcache.head {
            if (*b).dev == dev && (*b).blockno == blockno {
                (*b).refcnt = (*b).refcnt.wrapping_add(1);
                drop(bcache);
                (*b).lock.acquire();
                return b;
            }
            b = (*b).next
        }

        // Not cached; recycle an unused buffer.
        b = bcache.head.prev;
        while b != &mut bcache.head {
            if (*b).refcnt == 0 {
                (*b).dev = dev;
                (*b).blockno = blockno;
                (*b).valid = false;
                (*b).refcnt = 1;
                drop(bcache);
                (*b).lock.acquire();
                return b;
            }
            b = (*b).prev
        }
        panic!("get: no buffers");
    }
    /// Return a locked buf with the contents of the indicated block.
    pub unsafe fn read(dev: u32, blockno: u32) -> *mut Self {
        let b: *mut Self = Buf::get(dev, blockno);
        if !(*b).valid {
            virtio_disk_rw(b, false);
            (*b).valid = true
        }
        b
    }
}
