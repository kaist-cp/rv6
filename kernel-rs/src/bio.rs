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
    arena::{Arena, ArenaObject, MruArena, MruEntry, Rc},
    kernel::kernel,
    param::{BSIZE, NBUF},
    proc::WaitChannel,
    sleeplock::Sleeplock,
    spinlock::Spinlock,
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

pub type Bcache = Spinlock<MruArena<BufEntry, NBUF>>;

pub type BufUnlocked<'s> = Rc<Bcache, &'s Bcache>;

pub struct Buf<'s> {
    inner: BufUnlocked<'s>,
}

impl<'s> Deref for Buf<'s> {
    type Target = BufUnlocked<'s>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Buf<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'s> Buf<'s> {
    pub fn deref_inner(&self) -> &BufInner {
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut BufInner {
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn unlock(self) -> BufUnlocked<'s> {
        unsafe {
            self.inner.inner.unlock();
            mem::transmute(self)
        }
    }
}

impl Drop for Buf<'_> {
    fn drop(&mut self) {
        unsafe {
            self.inner.inner.unlock();
        }
    }
}

impl Bcache {
    pub const fn zero() -> Self {
        const fn bcache_entry(_: usize) -> MruEntry<BufEntry> {
            MruEntry::new(BufEntry::zero())
        }

        Spinlock::new(
            "BCACHE",
            MruArena::new(array_const_fn_init![bcache_entry; 30]),
        )
    }

    /// Return a unlocked buf with the contents of the indicated block.
    pub fn buf(&self, dev: u32, blockno: u32) -> BufUnlocked<'_> {
        let inner = self
            .find_or_alloc(
                |buf| buf.dev == dev && buf.blockno == blockno,
                |buf| {
                    buf.dev = dev;
                    buf.blockno = blockno;
                    buf.inner.get_mut().valid = false;
                },
            )
            .expect("[BufGuard::new] no buffers");

        unsafe { Rc::from_unchecked(&kernel().bcache, inner) }
    }

    /// Retrieves BufUnlocked without increasing reference count.
    pub fn buf_unforget(&self, dev: u32, blockno: u32) -> Option<BufUnlocked<'_>> {
        let inner = self.unforget(|buf| buf.dev == dev && buf.blockno == blockno)?;

        Some(unsafe { Rc::from_unchecked(&kernel().bcache, inner) })
    }
}

impl<'s> BufUnlocked<'s> {
    pub fn lock(self) -> Buf<'s> {
        mem::forget(self.inner.lock());
        Buf { inner: self }
    }

    pub fn deref_inner(&self) -> &BufInner {
        unsafe { self.inner.get_mut_unchecked() }
    }

    pub fn deref_mut_inner(&mut self) -> &mut BufInner {
        unsafe { self.inner.get_mut_unchecked() }
    }
}
