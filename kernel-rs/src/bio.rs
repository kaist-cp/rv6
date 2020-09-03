use crate::{
    buf::{Buf, BufBlock},
    param::NBUF,
    sleeplock::SleepLockGuard,
    spinlock::RawSpinlock,
    virtio_disk::virtio_disk_rw,
};
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

/// Buffer which acquired sleeplock
pub struct BufGuard<'a> {
    pub guard: SleepLockGuard<'a, BufBlock>,
    pub entry: *mut Buf,
}

impl BufGuard<'_> {
    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        virtio_disk_rw(self as *mut BufGuard<'_>, true);
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    pub unsafe fn release(self) {
        let bcache = BCACHE.get_mut();
        let mut buf = &mut *self.entry;
        drop(self);
        bcache.lock.acquire();
        (*buf).refcnt = (*buf).refcnt.wrapping_sub(1);
        if (*buf).refcnt == 0 {
            // no one is waiting for it.
            (*(*buf).next).prev = (*buf).prev;
            (*(*buf).prev).next = (*buf).next;
            (*buf).next = bcache.head.next;
            (*buf).prev = &mut bcache.head;
            (*bcache.head.next).prev = buf as *mut Buf;
            bcache.head.next = buf as *mut Buf
        }
        bcache.lock.release();
    }
    pub unsafe fn pin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        (*self.entry).refcnt = (*self.entry).refcnt.wrapping_add(1);
        bcache.lock.release();
    }
    pub unsafe fn unpin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        (*self.entry).refcnt = (*self.entry).refcnt.wrapping_sub(1);
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
        (*b).data.initlock("buffer");
        (*bcache.head.next).prev = b;
        bcache.head.next = b;
    }
}

/// Look through buffer cache for block on device dev.
/// If not found, allocate a buffer.
/// In either case, return locked buffer.
unsafe fn bget(dev: u32, blockno: u32) -> BufGuard<'static> {
    let bcache = BCACHE.get_mut();

    bcache.lock.acquire();

    // Is the block already cached?
    let mut b: *mut Buf = bcache.head.next;
    while b != &mut bcache.head as *mut Buf {
        if (*b).dev == dev && (*b).blockno == blockno {
            (*b).refcnt = (*b).refcnt.wrapping_add(1);
            bcache.lock.release();
            return BufGuard {
                guard: (*b).data.lock(),
                entry: b as *mut Buf,
            };
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
            return BufGuard {
                guard: (*b).data.lock(),
                entry: b as *mut Buf,
            };
        }
        b = (*b).prev
    }
    panic!("bget: no buffers");
}

/// Return a locked buf with the contents of the indicated block.
pub unsafe fn bread(dev: u32, blockno: u32) -> BufGuard<'static> {
    let mut b = bget(dev, blockno);
    if (*b.entry).valid == 0 {
        virtio_disk_rw(&mut b as *mut BufGuard<'_>, false);
        (*b.entry).valid = 1
    }
    b
}
