use crate::{buf::Buf, param::NBUF, spinlock::RawSpinlock, virtio_disk::virtio_disk_rw};
use core::mem::MaybeUninit;

/// Buffer cache.
///
/// The buffer cache is a linked list of buf structures holding cached copies of disk block
/// contents.  Caching disk blocks in memory reduces the number of disk reads and also provides a
/// synchronization point for disk blocks used by multiple processes.
///
/// Interface:
/// * To get a buffer for a particular disk block, call bread.
/// * After changing buffer data, call bwrite to write it to disk.
/// * When done with the buffer, call release.
/// * Do not use the buffer after calling release.
/// * Only one process at a time can use a buffer, so do not keep them longer than necessary.
struct Bcache {
    lock: RawSpinlock,
    buf: [Buf; NBUF],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    head: Buf,
}

static mut BCACHE: MaybeUninit<Bcache> = MaybeUninit::uninit();

impl Buf {
    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        if (*self).lock.holding() == 0 {
            panic!("bwrite");
        }
        virtio_disk_rw(self, 1);
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    pub unsafe fn release(&mut self) {
        let bcache = BCACHE.get_mut();

        if (*self).lock.holding() == 0 {
            panic!("brelease");
        }
        (*self).lock.release();
        bcache.lock.acquire();
        (*self).refcnt = (*self).refcnt.wrapping_sub(1);
        if (*self).refcnt == 0 {
            // no one is waiting for it.
            (*(*self).next).prev = (*self).prev;
            (*(*self).prev).next = (*self).next;
            (*self).next = bcache.head.next;
            (*self).prev = &mut bcache.head;
            (*bcache.head.next).prev = self;
            bcache.head.next = self
        }
        bcache.lock.release();
    }
    pub unsafe fn pin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        (*self).refcnt = (*self).refcnt.wrapping_add(1);
        bcache.lock.release();
    }
    pub unsafe fn unpin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        (*self).refcnt = (*self).refcnt.wrapping_sub(1);
        bcache.lock.release();
    }
}

pub unsafe fn binit() {
    let bcache = BCACHE.get_mut();

    bcache.lock.initlock("bcache");

    // Create linked list of buffers
    bcache.head.prev = &mut bcache.head;
    bcache.head.next = &mut bcache.head;
    for b in &mut bcache.buf[..] {
        (*b).next = bcache.head.next;
        (*b).prev = &mut bcache.head;
        (*b).lock.initlock(b"buffer\x00" as *const u8 as *mut u8);
        (*bcache.head.next).prev = b;
        bcache.head.next = b;
    }
}

/// Look through buffer cache for block on device dev.
/// If not found, allocate a buffer.
/// In either case, return locked buffer.
unsafe fn bget(dev: u32, blockno: u32) -> *mut Buf {
    let bcache = BCACHE.get_mut();

    bcache.lock.acquire();

    // Is the block already cached?
    let mut b: *mut Buf = bcache.head.next;
    while b != &mut bcache.head as *mut Buf {
        if (*b).dev == dev && (*b).blockno == blockno {
            (*b).refcnt = (*b).refcnt.wrapping_add(1);
            bcache.lock.release();
            (*b).lock.acquire();
            return b;
        }
        b = (*b).next
    }

    // Not cached; recycle an unused buffer.
    b = bcache.head.prev;
    while b != &mut bcache.head as *mut Buf {
        if (*b).refcnt == 0 {
            (*b).dev = dev;
            (*b).blockno = blockno;
            (*b).valid = 0;
            (*b).refcnt = 1;
            bcache.lock.release();
            (*b).lock.acquire();
            return b;
        }
        b = (*b).prev
    }
    panic!("bget: no buffers");
}

/// Return a locked buf with the contents of the indicated block.
pub unsafe fn bread(dev: u32, blockno: u32) -> *mut Buf {
    let mut b: *mut Buf = bget(dev, blockno);
    if (*b).valid == 0 {
        virtio_disk_rw(b, 0);
        (*b).valid = 1
    }
    b
}
