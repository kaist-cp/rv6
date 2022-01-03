// TODO: remove it
#![allow(unused_variables)]
#![allow(dead_code)]

use core::ops::Deref;

use kernel_aam::strong_pin::StrongPin;
use spin::Once;

use super::{FcntlFlags, FileSystem, Inode, InodeGuard, InodeType, Path, RcInode, Stat, Tx};
use crate::proc::KernelCtx;

mod inode;
mod superblock;

pub use inode::InodeInner;
pub use superblock::Superblock;

pub struct Lfs {
    /// Initializing superblock should run only once because forkret() calls FileSystem::init().
    /// There should be one superblock per disk device, but we run with only one device.
    superblock: Once<Superblock>,

    /// In-memory inode map.
    imap: (),
}

impl Lfs {
    pub const fn new() -> Self {
        Self {
            superblock: Once::new(),
            imap: (),
        }
    }

    fn superblock(&self) -> &Superblock {
        self.superblock.get().expect("superblock")
    }
}

impl FileSystem for Lfs {
    type Dirent = ();
    type InodeInner = InodeInner;

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    fn root(self: StrongPin<'_, Self>) -> RcInode<Self> {
        todo!()
    }

    fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Self>, ()> {
        todo!()
    }

    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }

    fn unlink(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }

    fn create<F, T>(
        self: StrongPin<'_, Self>,
        path: &Path,
        typ: InodeType,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
        f: F,
    ) -> Result<(RcInode<Self>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self>) -> T,
    {
        todo!()
    }

    fn open(
        self: StrongPin<'_, Self>,
        path: &Path,
        omode: FcntlFlags,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        todo!()
    }

    fn chdir(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self>,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }

    fn tx_begin(&self, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    unsafe fn tx_end(&self, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    #[inline]
    fn inode_read<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &[u8], &mut K) -> Result<(), ()>,
    >(
        guard: &mut InodeGuard<'_, Self>,
        off: u32,
        n: u32,
        f: F,
        k: K,
    ) -> Result<usize, ()> {
        todo!()
    }

    fn inode_write<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &mut [u8], &mut K) -> Result<(), ()>,
    >(
        guard: &mut InodeGuard<'_, Self>,
        off: u32,
        n: u32,
        f: F,
        tx: &Tx<'_, Lfs>,
        k: K,
    ) -> Result<usize, ()> {
        todo!()
    }

    fn inode_trunc(guard: &mut InodeGuard<'_, Self>, tx: &Tx<'_, Self>, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    fn inode_lock<'a>(inode: &'a Inode<Self>, ctx: &KernelCtx<'_, '_>) -> InodeGuard<'a, Self> {
        todo!()
    }

    fn inode_finalize<'a, 'id: 'a>(
        inode: &mut Inode<Self>,
        tx: &'a Tx<'a, Self>,
        ctx: &'a KernelCtx<'id, 'a>,
    ) {
        todo!()
    }

    fn inode_stat(inode: &Inode<Self>, ctx: &KernelCtx<'_, '_>) -> Stat {
        todo!()
    }
}
