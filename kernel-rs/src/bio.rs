//! Buffer cache.
//!
//! The buffer cache is a linked list of buf structures holding cached copies of disk block
//! contents.  Caching disk blocks in memory reduces the number of disk reads and also provides a
//! synchronization point for disk blocks used by multiple processes.
//!
//! Interface:
//! * To get a buffer for a particular disk block, call read.
//! * After changing buffer data, call bwrite to write it to disk.
//! * When done with the buffer, call release.
//! * Do not use the buffer after calling release.
//! * Only one process at a time can use a buffer, so do not keep them longer than necessary.

use crate::{buf::Buf, param::NBUF, spinlock::Spinlock};

struct Bcache {
    pub buf: [Buf; NBUF],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    pub head: Buf,
}

static mut BCACHE: Spinlock<Bcache> = Spinlock::new("BCACHE", Bcache::zeroed());

impl Bcache {
    // TODO:transient measure.
    const fn zeroed() -> Self {
        Self {
            buf: [Buf::zeroed(); NBUF],
            head: Buf::zeroed(),
        }
    }

    fn init(&mut self) {
        // Create linked list of buffers.
        self.head.prev = &mut self.head;
        self.head.next = &mut self.head;
        for b in &mut self.buf[..] {
            b.next = self.head.next;
            b.prev = &mut self.head;
            b.lock.initlock("buffer");
            unsafe {
                (*self.head.next).prev = b;
            }
            self.head.next = b;
        }
    }

    /// Look through buffer cache for block on device dev.
    /// If not found, allocate a buffer.
    /// In either case, return locked buffer.
    unsafe fn get(&mut self, dev: u32, blockno: u32) -> *mut Buf {
        // Is the block already cached?
        let mut b: *mut Buf = self.head.next;
        while b != &mut self.head {
            if (*b).dev == dev && (*b).blockno == blockno {
                (*b).refcnt = (*b).refcnt.wrapping_add(1);
                return b;
            }
            b = (*b).next
        }

        // Not cached; recycle an unused buffer.
        b = self.head.prev;
        while b != &mut self.head {
            if (*b).refcnt == 0 {
                (*b).dev = dev;
                (*b).blockno = blockno;
                (*b).inner.valid = false;
                (*b).refcnt = 1;
                return b;
            }
            b = (*b).prev
        }
        panic!("get: no buffers");
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    unsafe fn release(&mut self, buf: &mut Buf) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
        if buf.refcnt == 0 {
            // No one is waiting for it.
            (*buf.next).prev = buf.prev;
            (*buf.prev).next = buf.next;
            buf.next = self.head.next;
            buf.prev = &mut self.head;
            (*self.head.next).prev = buf;
            self.head.next = buf
        }
    }

    unsafe fn pin(&mut self, buf: &mut Buf) {
        buf.refcnt = buf.refcnt.wrapping_add(1);
    }

    unsafe fn unpin(&mut self, buf: &mut Buf) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
    }
}

pub unsafe fn binit() {
    let mut bcache = BCACHE.lock();
    bcache.init();
}

pub unsafe fn bget(dev: u32, blockno: u32) -> *mut Buf {
    let buf = BCACHE.lock().get(dev, blockno);
    (*buf).lock.acquire();
    buf
}

pub unsafe fn brelease(buf: &mut Buf) {
    if buf.lock.holding() == 0 {
        panic!("brelease");
    }
    buf.lock.release();
    let mut bcache = BCACHE.lock();
    bcache.release(buf);
}

pub unsafe fn bpin(buf: &mut Buf) {
    let mut bcache = BCACHE.lock();
    bcache.pin(buf);
    drop(bcache);
}

pub unsafe fn bunpin(buf: &mut Buf) {
    let mut bcache = BCACHE.lock();
    bcache.unpin(buf);
    drop(bcache);
}
