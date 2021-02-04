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
    param::{BSIZE, NBUF},
    proc::WaitChannel,
    sleeplock::Sleeplock,
    spinlock::Spinlock,
};

use array_macro::array;
use core::mem::{self, ManuallyDrop};
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

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

/// Type that actually stores the buffer cache.
pub type BcacheInner = MruArena<BufEntry, NBUF>;
/// Type that provides a pinned mutable reference of the buffer cache
/// to the outside.
// TODO: 'static?
pub type Bcache = Spinlock<Pin<&'static mut MruArena<BufEntry, NBUF>>>;

/// We can consider it as BufEntry.
pub type BufUnlocked<'s> = Rc<'s, Bcache, &'s Bcache>;

/// # Safety
///
/// (inner: BufEntry).inner is locked.
pub struct Buf<'s> {
    inner: ManuallyDrop<BufUnlocked<'s>>,
}

impl<'s> Buf<'s> {
    pub fn deref_inner(&self) -> &BufInner {
        // It is safe becuase inner.inner is locked.
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut BufInner {
        // It is safe becuase inner.inner is locked and &mut self is exclusive.
        unsafe { self.inner.inner.get_mut_unchecked() }
    }

    pub fn unlock(mut self) -> BufUnlocked<'s> {
        // It is safe because this method consumes self and self.inner will not
        // be used again.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        // It is safe because this method consumes self.
        unsafe { inner.inner.unlock() };
        mem::forget(self);
        inner
    }
}

impl Deref for Buf<'_> {
    type Target = BufEntry;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Drop for Buf<'_> {
    fn drop(&mut self) {
        // It is safe because self will be dropped and self.inner will not be
        // used again.
        unsafe { ManuallyDrop::take(&mut self.inner).inner.unlock() };
    }
}

impl BcacheInner {
    /// # Safety
    ///
    /// The caller should make sure that `Bcache` never gets moved.
    pub const unsafe fn zero() -> Self {
        MruArena::new(array![_ => MruEntry::new(BufEntry::zero()); NBUF])
    }
}

impl Bcache {
    /// Return a unlocked buf with the contents of the indicated block.
    pub fn get_buf(&self, dev: u32, blockno: u32) -> BufUnlocked<'_> {
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

impl<'s> BufUnlocked<'s> {
    pub fn lock(self) -> Buf<'s> {
        mem::forget(self.inner.lock());
        Buf {
            inner: ManuallyDrop::new(self),
        }
    }
}
