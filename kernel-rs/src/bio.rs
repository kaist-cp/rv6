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

pub struct Bcache {
    pub buf: [Buf; NBUF],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    pub head: Buf,
}

pub static mut BCACHE: Spinlock<Bcache> = Spinlock::new("BCACHE", Bcache::zeroed());

impl Bcache {
    // TODO:transient measure.
    pub const fn zeroed() -> Self {
        Self {
            buf: [Buf::zeroed(); NBUF],
            head: Buf::zeroed(),
        }
    }

    pub fn init(&mut self) {
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
}

pub unsafe fn binit() {
    let mut bcache = BCACHE.lock();
    bcache.init();
}
