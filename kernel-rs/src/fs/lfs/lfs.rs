//! The `Lfs` struct. Also includes the `Checkpoint`, `SegManagerReadOnlyGuard`, and `ImapReadOnlyGuard` type.
#![allow(clippy::module_inception)]

use core::mem;
use core::ops::Deref;

use pin_project::pin_project;
use spin::Once;
use static_assertions::const_assert;

use super::{Imap, Itable, SegManager, SegTable, Superblock, Tx, TxManager};
use crate::{
    bio::BufData,
    hal::hal,
    lock::{SleepLock, SleepLockGuard, SleepableLock},
    param::IMAPSIZE,
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

#[pin_project]
pub struct Lfs {
    /// Initializing superblock should run only once because forkret() calls FileSystem::init().
    /// There should be one superblock per disk device, but we run with only one device.
    superblock: Once<Superblock>,

    /// In-memory inodes.
    #[pin]
    itable: Itable<Self>,

    /// The segment manager.
    segmanager: Once<SleepLock<SegManager>>,

    /// Imap.
    imap: Once<SleepLock<Imap>>,

    tx_manager: Once<SleepableLock<TxManager>>,
}

/// On-disk checkpoint structure.
#[repr(C)]
pub struct Checkpoint {
    imap: [u32; IMAPSIZE],
    segtable: SegTable,
    timestamp: u32,
}

impl<'s> From<&'s BufData> for &'s Checkpoint {
    fn from(b: &'s BufData) -> Self {
        const_assert!(mem::size_of::<Checkpoint>() <= mem::size_of::<BufData>());
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<Checkpoint>() == 0);
        unsafe { &*(b.as_ptr() as *const Checkpoint) }
    }
}

/// A read-only guard of a `SegManager`.
/// Must be `free`d when done using it.
pub struct SegManagerReadOnlyGuard<'s>(SleepLockGuard<'s, SegManager>);

impl SegManagerReadOnlyGuard<'_> {
    pub fn free(self, ctx: &KernelCtx<'_, '_>) {
        self.0.free(ctx);
    }
}

impl Deref for SegManagerReadOnlyGuard<'_> {
    type Target = SegManager;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A read-only guard of a `SegManager`.
/// Must be `free`d when done using it.
pub struct ImapReadOnlyGuard<'s>(SleepLockGuard<'s, Imap>);

impl ImapReadOnlyGuard<'_> {
    pub fn free(self, ctx: &KernelCtx<'_, '_>) {
        self.0.free(ctx);
    }
}

impl Deref for ImapReadOnlyGuard<'_> {
    type Target = Imap;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Tx<'_, Lfs> {
    /// Acquires the lock on the `SegManager` and returns the lock guard.
    /// Use this to write blocks to the segment.
    /// Note that you must `free` the guard when done using it.
    pub fn segmanager(&self, ctx: &KernelCtx<'_, '_>) -> SleepLockGuard<'_, SegManager> {
        self.fs.segmanager.get().expect("segmanager").lock(ctx)
    }

    /// Acquires the lock on the `Imap` and returns the lock guard.
    /// Use this to mutate the `Imap`.
    /// Note that you must `free` the guard when done using it.
    pub fn imap(&self, ctx: &KernelCtx<'_, '_>) -> SleepLockGuard<'_, Imap> {
        self.fs.imap.get().expect("imap").lock(ctx)
    }
}

impl Lfs {
    pub const fn new() -> Self {
        Self {
            superblock: Once::new(),
            itable: Itable::<Self>::new_itable(),
            segmanager: Once::new(),
            imap: Once::new(),
            tx_manager: Once::new(),
        }
    }

    /// Returns a reference to the `Superblock`.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn superblock(&self) -> &Superblock {
        self.superblock.get().expect("superblock")
    }

    #[allow(clippy::needless_lifetimes)]
    /// Returns a reference to the `Itable`.
    pub fn itable<'s>(self: StrongPin<'s, Self>) -> StrongPin<'s, Itable<Self>> {
        unsafe { StrongPin::new_unchecked(&self.as_pin().get_ref().itable) }
    }

    /// Acquires the lock on the `SegManager` and returns a read-only guard.
    /// Note that you must `free` the guard when done using it.
    ///
    /// # Note
    ///
    /// If you need a writable lock guard, use `Tx::segmanager` instead.
    /// Note that this means you must have started a transaction.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn segmanager(&self, ctx: &KernelCtx<'_, '_>) -> SegManagerReadOnlyGuard<'_> {
        SegManagerReadOnlyGuard(self.segmanager.get().expect("segmanager").lock(ctx))
    }

    /// Returns a raw pointer to the `SegManager`.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn segmanager_raw(&self) -> *mut SegManager {
        self.segmanager.get().expect("segmanager").get_mut_raw()
    }

    /// Acquires the lock on the `Imap` and returns a read-only guard.
    /// Note that you must `free` the guard when done using it.
    ///
    /// # Note
    ///
    /// If you need a writable lock guard, use `Tx::imap` instead.
    /// Note that this means you must have started a transaction.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn imap(&self, ctx: &KernelCtx<'_, '_>) -> ImapReadOnlyGuard<'_> {
        ImapReadOnlyGuard(self.imap.get().expect("imap").lock(ctx))
    }

    /// Returns a raw pointer to the `Imap`.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn imap_raw(&self) -> *mut Imap {
        self.imap.get().expect("imap").get_mut_raw()
    }

    /// Acquires the lock on the `TxManager` and returns a lock guard.
    ///
    /// # Panic
    ///
    /// Panics if `self` is not initialized.
    pub fn tx_manager(&self) -> &SleepableLock<TxManager> {
        self.tx_manager.get().expect("tx_manager")
    }

    /// Initializes `self`.
    /// Does nothing if already initialized.
    pub fn initialize(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        if !self.superblock.is_completed() {
            // Load the superblock.
            let buf = hal().disk().read(dev, 1, ctx);
            let superblock = self.superblock.call_once(|| Superblock::new(&buf));
            buf.free(ctx);

            // Load the checkpoint.
            let (bno1, bno2) = superblock.get_chkpt_block_no();
            let buf1 = hal().disk().read(dev, bno1, ctx);
            let chkpt1: &Checkpoint = buf1.data().into();
            let buf2 = hal().disk().read(dev, bno2, ctx);
            let chkpt2: &Checkpoint = buf2.data().into();

            let (chkpt, timestamp, stored_at_first) = if chkpt1.timestamp > chkpt2.timestamp {
                (chkpt1, chkpt1.timestamp, true)
            } else {
                (chkpt2, chkpt2.timestamp, false)
            };

            let segtable = chkpt.segtable;
            let imap = chkpt.imap;
            // let timestamp = chkpt.timestamp;
            buf1.free(ctx);
            buf2.free(ctx);

            // Load other components using the checkpoint content.
            let _ = self.segmanager.call_once(|| {
                SleepLock::new(
                    "segment",
                    SegManager::new(dev, segtable, superblock.nsegments()),
                )
            });
            let _ = self
                .imap
                .call_once(|| SleepLock::new("imap", Imap::new(dev, superblock.ninodes(), imap)));
            let _ = self.tx_manager.call_once(|| {
                SleepableLock::new(
                    "tx_manager",
                    TxManager::new(dev, stored_at_first, timestamp),
                )
            });
        }
    }

    /// Commits the checkpoint at the checkpoint region.
    /// If `first` is `true`, writes it at the first checkpoint region. Otherwise, writes at the second region.
    pub fn commit_checkpoint(
        &self,
        first: bool,
        timestamp: u32,
        seg: &SegManager,
        imap: &Imap,
        dev: u32,
        ctx: &KernelCtx<'_, '_>,
    ) {
        let (bno1, bno2) = self.superblock().get_chkpt_block_no();
        let block_no = if first { bno1 } else { bno2 };

        let mut buf = ctx.kernel().bcache().get_buf_and_clear(dev, block_no, ctx);
        let chkpt = unsafe { &mut *(buf.data_mut().as_ptr() as *mut Checkpoint) };
        chkpt.segtable = seg.dsegtable();
        chkpt.imap = imap.dimap();
        chkpt.timestamp = timestamp;
        hal().disk().write(&mut buf, ctx);
        buf.free(ctx);
    }
}
