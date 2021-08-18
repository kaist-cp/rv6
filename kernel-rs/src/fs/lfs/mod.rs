// TODO: remove it
#![allow(unused_variables)]
#![allow(dead_code)]

use spin::Once;

use super::{FcntlFlags, FileSystem, Inode, InodeGuard, InodeType, Path, RcInode, Tx};
use crate::{proc::KernelCtx, util::strong_pin::StrongPin};

mod inode;
mod superblock;

pub use inode::I;
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
    type InodeInner = I;

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    fn root(self: StrongPin<'_, Self>) -> RcInode<Self::InodeInner> {
        todo!()
    }

    fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Self::InodeInner>, ()> {
        todo!()
    }

    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self::InodeInner>,
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
    ) -> Result<(RcInode<Self::InodeInner>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self::InodeInner>) -> T,
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
        inode: RcInode<Self::InodeInner>,
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
}
