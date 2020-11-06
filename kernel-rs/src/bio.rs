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

use crate::{
    arena::{Arena, ArenaObject, MruArena, Rc},
    fs::BSIZE,
    kernel::kernel,
    param::NBUF,
    proc::WaitChannel,
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    virtio_disk::Disk,
};

use core::mem;
use core::ops::{Deref, DerefMut};

pub struct BufEntry {
    dev: u32,
    pub blockno: u32,

    /// WaitChannel saying virtio_disk request is done.
    pub vdisk_request_waitchannel: WaitChannel,

    pub inner: Sleeplock<BufInner>,
}

impl BufEntry {
    pub const fn zero() -> Self {
        Self {
            dev: 0,
            blockno: 0,
            vdisk_request_waitchannel: WaitChannel::new(),
            inner: Sleeplock::new("buffer", BufInner::zero()),
        }
    }
}

impl ArenaObject for BufEntry {
    fn finalize<'s, A: Arena>(&'s mut self, _guard: &'s mut A::Guard<'_>) {
        // The buffer contents should have been written. Does nothing.
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

#[derive(Clone)]
pub struct BcacheTag {}

impl Deref for BcacheTag {
    type Target = Spinlock<MruArena<BufEntry, NBUF>>;

    fn deref(&self) -> &Self::Target {
        &kernel().bcache
    }
}

pub type BufUnlocked = Rc<<BcacheTag as Deref>::Target, BcacheTag>;

pub struct Buf {
    inner: BufUnlocked,
}

impl Deref for Buf {
    type Target = BufUnlocked;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Buf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Buf {
    pub fn deref_inner(&self) -> &BufInner {
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut BufInner {
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn unlock(self) -> BufUnlocked {
        unsafe {
            self.inner.inner.unlock();
            mem::transmute(self)
        }
    }

    /// Return a locked buf with the contents of the indicated block.
    pub fn new(dev: u32, blockno: u32) -> Self {
        let buf = BufUnlocked::new(dev, blockno);
        buf.lock()
    }

    /// Write self's contents to disk.  Must be locked.
    pub unsafe fn write(&mut self) {
        Disk::virtio_rw(&mut kernel().disk.lock(), self, true);
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        unsafe {
            self.inner.inner.unlock();
        }
    }
}

impl BufUnlocked {
    /// Return a unlocked buf with the contents of the indicated block.
    pub fn new(dev: u32, blockno: u32) -> Self {
        BcacheTag {}
            .find_or_alloc(
                |buf| buf.dev == dev && buf.blockno == blockno,
                |buf| {
                    buf.dev = dev;
                    buf.blockno = blockno;
                    buf.inner.get_mut().valid = false;
                },
            )
            .expect("[BufGuard::new] no buffers")
    }

    /// Retrieves BufUnlocked without increasing reference count.
    pub fn unforget(dev: u32, blockno: u32) -> Option<Self> {
        BcacheTag {}.unforget(|buf| buf.dev == dev && buf.blockno == blockno)
    }

    pub fn lock(self) -> Buf {
        mem::forget(self.inner.lock());
        let mut result = Buf { inner: self };

        if !result.deref_inner().valid {
            unsafe {
                Disk::virtio_rw(&mut kernel().disk.lock(), &mut result, false);
            }
            result.deref_mut_inner().valid = true;
        }

        result
    }

    pub fn deref_inner(&self) -> &BufInner {
        unsafe { self.inner.get_mut_unchecked() }
    }

    pub fn deref_mut_inner(&mut self) -> &mut BufInner {
        unsafe { self.inner.get_mut_unchecked() }
    }
}
