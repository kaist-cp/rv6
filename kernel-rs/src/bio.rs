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

use crate::{fs::BSIZE, kernel::kernel, proc::WaitChannel, sleeplock::Sleeplock};
use crate::{param::NBUF, virtio_disk::virtio_disk_rw};

use core::mem;
use core::ops::Deref;
use core::ptr;

struct ListEntry {
    prev: *mut Self,
    next: *mut Self,
}

pub struct MruEntry<T> {
    refcnt: u32,
    /// LRU cache list.
    list_entry: ListEntry,
    pub data: T,
}

pub struct BufData {
    dev: u32,
    pub blockno: u32,
    /// WaitChannel saying virtio_disk request is done.
    pub vdisk_request_waitchannel: WaitChannel,

    pub inner: Sleeplock<BufInner>,
}

pub type BufEntry = MruEntry<BufData>;

impl ListEntry {
    pub const fn new() -> Self {
        Self {
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }
}

impl<T> MruEntry<T> {
    const fn new(data: T) -> Self {
        Self {
            refcnt: 0,
            list_entry: ListEntry::new(),
            data,
        }
    }
}

impl BufEntry {
    pub const fn zero() -> Self {
        MruEntry::new(BufData {
            dev: 0,
            blockno: 0,
            vdisk_request_waitchannel: WaitChannel::new(),
            inner: Sleeplock::new("buffer", BufInner::zero()),
        })
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
    const fn zero() -> Self {
        Self {
            valid: false,
            disk: false,
            data: [0; BSIZE],
        }
    }
}

pub struct Bcache {
    pub buf: [BufEntry; NBUF],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    head: ListEntry,
}

impl Bcache {
    pub const fn zero() -> Self {
        Self {
            buf: [BufEntry::zero(); NBUF],
            head: ListEntry::new(),
        }
    }

    pub fn init(&mut self) {
        // Create linked list of buffers.
        self.head.prev = &mut self.head;
        self.head.next = &mut self.head;
        for b in &mut self.buf[..] {
            b.list_entry.next = self.head.next;
            b.list_entry.prev = &mut self.head;
            unsafe {
                (*self.head.next).prev = &mut b.list_entry;
            }
            self.head.next = &mut b.list_entry;
        }
    }

    unsafe fn unforget(&mut self, dev: u32, blockno: u32) -> *mut BufEntry {
        // Is the block already cached?
        let mut e = self.head.next;
        while e != &mut self.head {
            let b: *mut BufEntry = (e as usize - offset_of!(BufEntry, list_entry)) as *mut _;

            if (*b).data.dev == dev && (*b).data.blockno == blockno {
                return b;
            }
            e = (*e).next;
        }

        ptr::null_mut()
    }

    /// Look through buffer cache for block on device dev.
    /// If not found, allocate a buffer.
    /// In either case, return locked buffer.
    unsafe fn get(&mut self, dev: u32, blockno: u32) -> *mut BufEntry {
        // Is the block already cached?
        let b = self.unforget(dev, blockno);
        if !b.is_null() {
            (*b).refcnt = (*b).refcnt.wrapping_add(1);
            return b;
        }

        // Not cached; recycle an unused buffer.
        let mut e = self.head.prev;
        while e != &mut self.head {
            let b: *mut BufEntry = (e as usize - offset_of!(BufEntry, list_entry)) as *mut _;

            if (*b).refcnt == 0 {
                (*b).data.dev = dev;
                (*b).data.blockno = blockno;
                (*b).data.inner.get_mut().valid = false;
                (*b).refcnt = 1;
                return b;
            }
            e = (*e).prev;
        }

        ptr::null_mut()
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    unsafe fn release(&mut self, buf: &mut BufEntry) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
        if buf.refcnt == 0 {
            // No one is waiting for it.
            (*buf.list_entry.next).prev = buf.list_entry.prev;
            (*buf.list_entry.prev).next = buf.list_entry.next;
            buf.list_entry.next = self.head.next;
            buf.list_entry.prev = &mut self.head;
            (*self.head.next).prev = &mut buf.list_entry;
            self.head.next = &mut buf.list_entry;
        }
    }

    unsafe fn unpin(&mut self, buf: &mut BufEntry) {
        buf.refcnt = buf.refcnt.wrapping_sub(1);
    }
}

pub struct Buf {
    /// Assumption: the `ptr.inner` lock is held.
    ptr: *mut BufEntry,
}

pub struct BufUnlocked {
    /// Assumption: the `ptr.inner` lock is unheld.
    ptr: *mut BufEntry,
}

impl BufUnlocked {
    pub fn lock(self) -> Buf {
        unsafe {
            mem::forget((*self.ptr).data.inner.lock());
        }
        let result = Buf { ptr: self.ptr };
        mem::forget(self);
        result
    }

    pub unsafe fn from_blockno(dev: u32, blockno: u32) -> Self {
        let ptr = kernel().bcache.lock().unforget(dev, blockno);
        debug_assert!(!ptr.is_null());
        Self { ptr }
    }
}

impl Drop for BufUnlocked {
    fn drop(&mut self) {
        unsafe {
            let buf = &mut *self.ptr;
            let mut bcache = kernel().bcache.lock();
            bcache.release(buf);
        }
    }
}

impl Buf {
    /// Return a locked buf with the contents of the indicated block.
    pub fn new(dev: u32, blockno: u32) -> Self {
        unsafe {
            let ptr = kernel().bcache.lock().get(dev, blockno);
            debug_assert!(!ptr.is_null(), "[Buf::new] no buffers");

            mem::forget((*ptr).data.inner.lock());
            let mut result = Self { ptr };

            if !result.deref_inner().valid {
                virtio_disk_rw(&mut result, false);
                result.deref_mut_inner().valid = true;
            }

            result
        }
    }

    pub fn pin(self) -> BufUnlocked {
        unsafe {
            let buf = &mut *self.ptr;
            buf.data.inner.unlock();
        }
        let result = BufUnlocked { ptr: self.ptr };
        mem::forget(self);
        result
    }

    pub unsafe fn unpin(&mut self) {
        let mut bcache = kernel().bcache.lock();
        let buf = &mut *self.ptr;
        bcache.unpin(buf);
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        virtio_disk_rw(self, true);
    }

    pub fn deref_inner(&self) -> &BufInner {
        unsafe { (*self.ptr).data.inner.get_mut_unchecked() }
    }

    pub fn deref_mut_inner(&mut self) -> &mut BufInner {
        unsafe { (*self.ptr).data.inner.get_mut_unchecked() }
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        unsafe {
            let buf = &mut *self.ptr;
            buf.data.inner.unlock();
            let mut bcache = kernel().bcache.lock();
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

impl Deref for BufUnlocked {
    type Target = BufEntry;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}
