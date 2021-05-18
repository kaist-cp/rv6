// TODO: remove it
#![allow(unused_variables)]

use cstr_core::CStr;

use super::{FcntlFlags, FileSystem, Inode, InodeGuard, InodeType, Path, RcInode};
use crate::{
    arena::{Arena, ArenaObject},
    kernel::KernelRef,
    proc::KernelCtx,
};

pub struct InodeInner {}

impl ArenaObject for Inode<InodeInner> {
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>) {}
}

pub struct Lfs {}

impl FileSystem for Lfs {
    type Dirent = ();
    type InodeInner = InodeInner;
    type Tx<'s> = &'s ();

    fn init_disk(&mut self) {
        todo!()
    }

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        todo!()
    }

    fn intr(&self, kernel: KernelRef<'_, '_>) {
        todo!()
    }

    fn begin_tx(&self, ctx: &KernelCtx<'_, '_>) -> Self::Tx<'_> {
        todo!()
    }

    fn link(
        &self,
        oldname: &CStr,
        newname: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }

    fn unlink(
        &self,
        filename: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }

    fn create<F, T>(
        &self,
        path: &Path,
        typ: InodeType,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
        f: F,
    ) -> Result<(RcInode<Self::InodeInner>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self::InodeInner>) -> T,
    {
        todo!()
    }

    fn open(
        &self,
        name: &Path,
        omode: FcntlFlags,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        todo!()
    }

    fn chdir(
        &self,
        dirname: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        todo!()
    }
}
