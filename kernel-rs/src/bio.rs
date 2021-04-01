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

use core::mem::{self, ManuallyDrop};
use core::ops::{Deref, DerefMut};

use crate::{
    arena::{Arena, ArenaObject, MruArena, Rc},
    lock::{Sleeplock, Spinlock},
    param::{BSIZE, NBUF},
    proc::WaitChannel,
};

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

#[rustfmt::skip] // Need this if lower than rustfmt 1.4.34
impl const Default for BufEntry {
    fn default() -> Self {
        Self::zero()
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
    pub data: BufData,
}

// Data in Buf may be assumed to be u32, so the data field in Buf must have
// an alignment of 4 bytes. Due to the align(4) modifier, BufData has an
// alignment of 4 bytes.
#[repr(align(4))]
pub struct BufData {
    pub inner: [u8; BSIZE],
}

impl Deref for BufData {
    type Target = [u8; BSIZE];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for BufData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl BufInner {
    const fn zero() -> Self {
        Self {
            valid: false,
            disk: false,
            data: BufData { inner: [0; BSIZE] },
        }
    }
}

pub type Bcache = Spinlock<MruArena<BufEntry, NBUF>>;

/// A reference counted smart pointer to a `BufEntry`.
pub type BufUnlocked = Rc<Bcache>;

/// A locked `BufEntry`.
///
/// # Safety
///
/// (inner: BufEntry).inner is locked.
pub struct Buf {
    inner: ManuallyDrop<BufUnlocked>,
}

impl Buf {
    pub fn deref_inner(&self) -> &BufInner {
        let entry: &BufEntry = &self.inner;
        // SAFETY: inner.inner is locked.
        unsafe { &*entry.inner.get_mut_raw() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut BufInner {
        let entry: &BufEntry = &self.inner;
        // SAFETY: inner.inner is locked and &mut self is exclusive.
        unsafe { &mut *entry.inner.get_mut_raw() }
    }

    pub fn unlock(mut self) -> BufUnlocked {
        // SAFETY: this method consumes self and self.inner will not be used again.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        // SAFETY: this method consumes self.
        unsafe { inner.inner.unlock() };
        mem::forget(self);
        inner
    }
}

impl Deref for Buf {
    type Target = BufEntry;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        // SAFETY: self will be dropped and self.inner will not be
        // used again.
        unsafe { ManuallyDrop::take(&mut self.inner).inner.unlock() };
    }
}

impl Bcache {
    /// # Safety
    ///
    /// The caller should make sure that `Bcache` never gets moved.
    pub const unsafe fn zero() -> Self {
        Spinlock::new("BCACHE", MruArena::<BufEntry, NBUF>::new())
    }

    /// Return a unlocked buf with the contents of the indicated block.
    pub fn get_buf(&self, dev: u32, blockno: u32) -> BufUnlocked {
        self.find_or_alloc(
            |buf| buf.dev == dev && buf.blockno == blockno,
            |buf| {
                buf.dev = dev;
                buf.blockno = blockno;
                buf.inner.get_mut().valid = false;
            },
        )
        .expect("[BufGuard::new] no buffers")
    }
}

impl BufUnlocked {
    pub fn lock(self) -> Buf {
        mem::forget(self.inner.lock());
        Buf {
            inner: ManuallyDrop::new(self),
        }
    }
}
