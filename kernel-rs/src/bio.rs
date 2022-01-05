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
use core::pin::Pin;

use kernel_aam::{
    arena::{Arena, ArenaObject, ArenaRc, MruArena},
    strong_pin::StrongPin,
};

use crate::lock::RawSpinLock;
use crate::{
    lock::SleepLock,
    param::{BSIZE, NBUF},
    proc::{KernelCtx, WaitChannel},
    util::memmove,
};

pub struct BufEntry {
    dev: u32,
    pub blockno: u32,

    /// WaitChannel saying virtio_disk request is done.
    pub vdisk_request_waitchannel: WaitChannel,

    pub inner: SleepLock<BufInner>,
}

impl BufEntry {
    pub const fn new() -> Self {
        Self {
            dev: 0,
            blockno: 0,
            vdisk_request_waitchannel: WaitChannel::new(),
            inner: SleepLock::new("buffer", BufInner::new()),
        }
    }
}

impl const Default for BufEntry {
    fn default() -> Self {
        Self::new()
    }
}

impl ArenaObject for BufEntry {
    type Ctx<'a, 'id: 'a> = ();

    #[allow(clippy::needless_lifetimes)]
    fn finalize<'a, 'id: 'a>(&mut self, _: ()) {
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

impl BufData {
    pub fn copy_from(&mut self, buf: &BufData) {
        memmove(&mut self.inner, &buf.inner);
    }
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
    const fn new() -> Self {
        Self {
            valid: false,
            disk: false,
            data: BufData { inner: [0; BSIZE] },
        }
    }
}

pub type BcacheArena = MruArena<BufEntry, RawSpinLock, NBUF>;

#[repr(transparent)]
pub struct Bcache(BcacheArena);

/// A reference counted smart pointer to a `BufEntry`.
pub struct BufUnlocked(ManuallyDrop<ArenaRc<BcacheArena>>);

/// A locked `BufEntry`.
///
/// # Safety
///
/// (inner: BufEntry).inner is locked.
pub struct Buf {
    inner: ManuallyDrop<BufUnlocked>,
}

impl BufUnlocked {
    pub fn lock(self, ctx: &KernelCtx<'_, '_>) -> Buf {
        mem::forget(self.inner.lock(ctx));
        Buf {
            inner: ManuallyDrop::new(self),
        }
    }
}

impl Deref for BufUnlocked {
    type Target = BufEntry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for BufUnlocked {
    fn drop(&mut self) {
        // SAFETY: `self` is being dropped.
        unsafe { ManuallyDrop::take(&mut self.0) }.free(());
    }
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

    pub fn unlock(mut self, ctx: &KernelCtx<'_, '_>) -> BufUnlocked {
        // SAFETY: this method consumes self and self.inner will not be used again.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        // SAFETY: this method consumes self.
        unsafe { inner.inner.unlock(ctx) };
        mem::forget(self);
        inner
    }

    pub fn free(self, ctx: &KernelCtx<'_, '_>) {
        let _ = self.unlock(ctx);
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
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Buf must never drop.");
    }
}

impl Bcache {
    /// # Safety
    ///
    /// Must be used only after initializing it with `MruArena::init`.
    pub const unsafe fn new_bcache() -> Self {
        unsafe {
            Self(MruArena::<BufEntry, RawSpinLock, NBUF>::new(
                RawSpinLock::new("BCACHE"),
            ))
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().0) }.init();
    }

    /// Return a unlocked buf with the contents of the indicated block.
    pub fn get_buf(self: StrongPin<'_, Self>, dev: u32, blockno: u32) -> BufUnlocked {
        let this = unsafe { StrongPin::new_unchecked(&self.as_pin().get_ref().0) };
        BufUnlocked(ManuallyDrop::new(
            this.find_or_alloc(
                |buf| buf.dev == dev && buf.blockno == blockno,
                |buf| {
                    buf.dev = dev;
                    buf.blockno = blockno;
                    buf.inner.get_mut().valid = false;
                },
            )
            .expect("[BufGuard::new] no buffers"),
        ))
    }
}
