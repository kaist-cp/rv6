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
use core::ops::{Deref, DerefMut};
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
            lock: Sleeplock::new("buffer"),
            refcnt: 0,
            vdisk_request_waitchannel: WaitChannel::new(),

            prev: ptr::null_mut(),
            next: ptr::null_mut(),

            inner: BufInner::zeroed(),
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
            self.head.next = buf;
        }
    }

    unsafe fn unpin(&mut self, buf: &mut Buf) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
    }
}

pub unsafe fn binit() {
    let mut bcache = BCACHE.lock();
    bcache.init();
}

pub struct BufHandle {
    ptr: *mut Buf,
}

impl BufHandle {
    /// Return a locked buf with the contents of the indicated block.
    pub fn new(dev: u32, blockno: u32) -> Self {
        unsafe {
            let ptr = BCACHE.lock().get(dev, blockno);
            (*ptr).lock.acquire();
            if !(*ptr).inner.valid {
                virtio_disk_rw(ptr, false);
                (*ptr).inner.valid = true;
            }
            Self { ptr }
        }
    }

    pub fn pin(self) {
        unsafe {
            let buf = &mut *self.ptr;
            if !buf.lock.holding() {
                panic!("BufHandle::drop");
            }
            buf.lock.release();
        }
        mem::forget(self);
    }

    pub fn unpin(&mut self) {
        unsafe {
            let mut bcache = BCACHE.lock();
            let buf = &mut *self.ptr;
            bcache.unpin(buf);
        }
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        if !(*self).lock.holding() {
            panic!("bwrite");
        }
        virtio_disk_rw(self.deref_mut(), true);
    }
}

impl Drop for BufHandle {
    fn drop(&mut self) {
        unsafe {
            let buf = &mut *self.ptr;
            if !buf.lock.holding() {
                panic!("BufHandle::drop");
            }
            buf.lock.release();
            let mut bcache = BCACHE.lock();
            bcache.release(buf);
        }
    }
}

impl Deref for BufHandle {
    type Target = Buf;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for BufHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}
