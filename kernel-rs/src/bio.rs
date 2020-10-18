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

use crate::{fs::BSIZE, proc::WaitChannel, sleeplock::Sleeplock};
use crate::{param::NBUF, spinlock::Spinlock, virtio_disk::virtio_disk_rw};

use core::mem;
use core::ops::Deref;
use core::ptr;

pub struct BufEntry {
    dev: u32,
    pub blockno: u32,
    refcnt: u32,
    /// WaitChannel saying virtio_disk request is done.
    pub vdisk_request_waitchannel: WaitChannel,

    /// LRU cache list.
    prev: *mut BufEntry,
    next: *mut BufEntry,

    pub inner: Sleeplock<BufInner>,
}

impl BufEntry {
    pub const fn zeroed() -> Self {
        Self {
            dev: 0,
            blockno: 0,
            refcnt: 0,
            vdisk_request_waitchannel: WaitChannel::new(),

            prev: ptr::null_mut(),
            next: ptr::null_mut(),

            inner: Sleeplock::new("buffer", BufInner::zeroed()),
        }
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
    const fn zeroed() -> Self {
        Self {
            valid: false,
            disk: false,
            data: [0; BSIZE],
        }
    }
}

struct Bcache {
    pub buf: [BufEntry; NBUF],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    pub head: BufEntry,
}

static mut BCACHE: Spinlock<Bcache> = Spinlock::new("BCACHE", Bcache::zeroed());

impl Bcache {
    // TODO:transient measure.
    const fn zeroed() -> Self {
        Self {
            buf: [BufEntry::zeroed(); NBUF],
            head: BufEntry::zeroed(),
        }
    }

    fn init(&mut self) {
        // Create linked list of buffers.
        self.head.prev = &mut self.head;
        self.head.next = &mut self.head;
        for b in &mut self.buf[..] {
            b.next = self.head.next;
            b.prev = &mut self.head;
            unsafe {
                (*self.head.next).prev = b;
            }
            self.head.next = b;
        }
    }

    /// Look through buffer cache for block on device dev.
    /// If not found, allocate a buffer.
    /// In either case, return locked buffer.
    unsafe fn get(&mut self, dev: u32, blockno: u32) -> *mut BufEntry {
        // Is the block already cached?
        let mut b: *mut BufEntry = self.head.next;
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
                (*b).inner.get_mut().valid = false;
                (*b).refcnt = 1;
                return b;
            }
            b = (*b).prev
        }
        panic!("get: no buffers");
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    unsafe fn release(&mut self, buf: &mut BufEntry) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
        if buf.refcnt == 0 {
            // No one is waiting for it.
            (*buf.next).prev = buf.prev;
            (*buf.prev).next = buf.next;
            buf.next = self.head.next;
            buf.prev = &mut self.head;
            (*self.head.next).prev = buf;
            self.head.next = buf;
        }
    }

    unsafe fn unpin(&mut self, buf: &mut BufEntry) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
    }
}

pub unsafe fn binit() {
    let mut bcache = BCACHE.lock();
    bcache.init();
}

pub struct Buf {
    /// Assumption: the `ptr.inner` lock is held.
    ptr: *mut BufEntry,
}

impl Buf {
    /// Return a locked buf with the contents of the indicated block.
    pub fn new(dev: u32, blockno: u32) -> Self {
        unsafe {
            let ptr = BCACHE.lock().get(dev, blockno);
            mem::forget((*ptr).inner.lock());
            let mut result = Self { ptr };

            if !result.deref_inner().valid {
                virtio_disk_rw(&mut result, false);
                result.deref_mut_inner().valid = true;
            }

            result
        }
    }

    pub fn pin(self) {
        unsafe {
            let buf = &mut *self.ptr;
            buf.inner.unlock();
        }
        mem::forget(self);
    }

    pub unsafe fn unpin(&mut self) {
        let mut bcache = BCACHE.lock();
        let buf = &mut *self.ptr;
        bcache.unpin(buf);
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        virtio_disk_rw(self, true);
    }

    pub fn deref_inner(&self) -> &BufInner {
        unsafe { (*self.ptr).inner.get_mut_unchecked() }
    }

    pub fn deref_mut_inner(&mut self) -> &mut BufInner {
        unsafe { (*self.ptr).inner.get_mut_unchecked() }
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        unsafe {
            let buf = &mut *self.ptr;
            buf.inner.unlock();
            let mut bcache = BCACHE.lock();
            bcache.release(buf);
        }
    }
}

impl Deref for Buf {
    type Target = BufEntry;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}
