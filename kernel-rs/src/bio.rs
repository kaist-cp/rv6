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

use crate::arena::ArenaRc;
use crate::util::strong_pin::StrongPin;
use crate::{
    arena::{Arena, ArenaObject, MruArena},
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

    inner: SleepLock<BufInner>,
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
    valid: bool,

    /// Does disk "own" buf?
    disk: bool,

    data: BufData,
}

// Data in Buf may be assumed to be u32, so the data field in Buf must have
// an alignment of 4 bytes. Due to the align(4) modifier, BufData has an
// alignment of 4 bytes.
#[repr(align(4))]
pub struct BufData {
    pub inner: [u8; BSIZE],
}

impl BufData {
    #[allow(dead_code)]
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

pub type Bcache = MruArena<BufEntry, NBUF>;

/// A reference counted smart pointer to a `BufEntry`.
/// Use `BufUnlocked::lock` to access the buffer's inner data.
#[derive(Clone)]
pub struct BufUnlocked(ManuallyDrop<ArenaRc<Bcache>>);

/// A locked `BufEntry`.
///
/// # Safety
///
/// (inner: BufEntry).inner is locked.
pub struct Buf {
    inner: ManuallyDrop<BufUnlocked>,
}

impl BufUnlocked {
    /// Returns a locked `Buf` after releasing the lock and consuming `self`.
    /// Use this to access the buffer's inner data.
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
    /// Returns a reference to the `BufInner`, which includes the buffer data
    /// and other fields. We can safely do this since we have the lock.
    fn deref_inner(&self) -> &BufInner {
        let entry: &BufEntry = &self.inner;
        // SAFETY: inner.inner is locked.
        unsafe { &*entry.inner.get_mut_raw() }
    }

    /// Returns a mutable reference to the `BufInner`, which includes the buffer data
    /// and other fields. We can safely do this since we have the lock.
    fn deref_inner_mut(&mut self) -> &mut BufInner {
        let entry: &BufEntry = &self.inner;
        // SAFETY: inner.inner is locked and &mut self is exclusive.
        unsafe { &mut *entry.inner.get_mut_raw() }
    }

    /// Returns a reference to the `Buf`'s inner data.
    pub fn data(&self) -> &BufData {
        &self.deref_inner().data
    }

    /// Returns a mutable reference to the `Buf`'s inner data.
    pub fn data_mut(&mut self) -> &mut BufData {
        &mut self.deref_inner_mut().data
    }

    /// Returns whether the data of this `Buf` is initialized or not.
    /// If it is not, you should initialize, such as by `data_mut` or `copy_from`,
    /// and then call `mark_initialized`.
    pub fn is_initialized(&self) -> bool {
        self.deref_inner().valid
    }

    /// Marks the `Buf`'s data as initialized.
    pub fn mark_initialized(&mut self) {
        self.deref_inner_mut().valid = true;
    }

    /// Returns a mutable reference to the `BufInner`'s `disk` field,
    /// which marks whether the buffer is owned by the disk or not.
    /// Usually you should use this only inside driver related code.
    pub fn disk_mut(&mut self) -> &mut bool {
        &mut self.deref_inner_mut().disk
    }

    /// Returns a new `BufUnlocked` without releasing the lock or consuming `self`.
    #[allow(dead_code)]
    pub fn create_unlocked(&self) -> BufUnlocked {
        unsafe { ManuallyDrop::take(&mut self.inner.clone()) }
    }

    /// Returns a `BufUnlocked` after releasing the lock and consuming `self`.
    pub fn unlock(mut self, ctx: &KernelCtx<'_, '_>) -> BufUnlocked {
        // SAFETY: this method consumes self and self.inner will not be used again.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        // SAFETY: this method consumes self.
        unsafe { inner.inner.unlock(ctx) };
        mem::forget(self);
        inner
    }

    /// Releases the lock and consumes `self`.
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
        unsafe { MruArena::<BufEntry, NBUF>::new("BCACHE") }
    }

    /// Return a unlocked buf with the contents of the indicated block.
    pub fn get_buf(self: StrongPin<'_, Self>, dev: u32, blockno: u32) -> BufUnlocked {
        BufUnlocked(ManuallyDrop::new(
            self.find_or_alloc(
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

    /// Returns a locked buf with its content all zeroed.
    pub fn get_buf_and_clear(
        self: StrongPin<'_, Self>,
        dev: u32,
        blockno: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Buf {
        let mut buf = self.get_buf(dev, blockno).lock(ctx);
        buf.data_mut().fill(0);
        buf.mark_initialized();
        buf
    }
}
