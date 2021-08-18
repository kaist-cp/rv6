use core::mem;
use core::ops::Deref;

use bitflags::bitflags;
use zerocopy::{AsBytes, FromBytes};

use crate::{
    arch::addr::UVAddr,
    arena::{ArenaObject, ArenaRc, ArrayArena},
    lock::SleepLock,
    param::NINODE,
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

mod lfs;
mod path;
mod stat;
mod ufs;

pub use lfs::Lfs;
pub use path::{FileName, Path};
pub use stat::Stat;
pub use ufs::Ufs;

bitflags! {
    pub struct FcntlFlags: i32 {
        const O_RDONLY = 0;
        const O_WRONLY = 0x1;
        const O_RDWR = 0x2;
        const O_CREATE = 0x200;
        const O_TRUNC = 0x400;
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
#[repr(i16)]
pub enum InodeType {
    None,
    Dir,
    File,
    Device { major: u16, minor: u16 },
}

/// InodeGuard implies that `SleepLock<InodeInner>` is held by current thread.
///
/// # Safety
///
/// `inode.inner` is locked.
// Every disk write operation must happen inside a transaction. Reading an
// opened file does not write anything on disk in any matter and thus does
// not need to happen inside a transaction. At the same time, it requires
// an InodeGuard. Therefore, InodeGuard does not have a FsTransaction field.
// Instead, every method that needs to be inside a transaction explicitly
// takes a FsTransaction value as an argument.
// https://github.com/kaist-cp/rv6/issues/328
pub struct InodeGuard<'a, I: InodeInner> {
    pub inode: &'a Inode<I>,
}

impl<I: InodeInner> Deref for InodeGuard<'_, I> {
    type Target = Inode<I>;

    fn deref(&self) -> &Self::Target {
        self.inode
    }
}

impl<I: InodeInner> InodeGuard<'_, I> {
    pub fn deref_inner(&self) -> &I {
        // SAFETY: self.inner is locked.
        unsafe { &*self.inner.get_mut_raw() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut I {
        // SAFETY: self.inner is locked and &mut self is exclusive.
        unsafe { &mut *self.inner.get_mut_raw() }
    }

    pub fn free(self, ctx: &KernelCtx<'_, '_>) {
        // SAFETY: self will be dropped.
        unsafe { self.inner.unlock(ctx) };
        core::mem::forget(self);
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return Ok(()) on success, Err(()) on failure.
    pub fn read_kernel<T: AsBytes + FromBytes>(
        &mut self,
        dst: &mut T,
        off: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let bytes = self.read_bytes_kernel(dst.as_bytes_mut(), off, ctx);
        if bytes == mem::size_of::<T>() {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return the number of bytes copied.
    pub fn read_bytes_kernel(
        &mut self,
        dst: &mut [u8],
        off: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> usize {
        I::read_internal(
            self,
            off,
            dst.len() as u32,
            |off, src, _| {
                dst[off as usize..off as usize + src.len()].clone_from_slice(src);
                Ok(())
            },
            ctx,
        )
        .expect("read: should never fail")
    }

    /// Copy data into virtual address `dst` of the current process by `n` bytes
    /// from the content of inode at offset `off`.
    /// Returns Ok(number of bytes copied) on success, Err(()) on failure due to
    /// accessing an invalid virtual address.
    pub fn read_user(
        &mut self,
        dst: UVAddr,
        off: u32,
        n: u32,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        I::read_internal(
            self,
            off,
            n,
            |off, src, ctx| {
                ctx.proc_mut()
                    .memory_mut()
                    .copy_out_bytes(dst + off as usize, src)
            },
            ctx,
        )
    }
}

/// Unlock and put the given inode.
impl<I: InodeInner> Drop for InodeGuard<'_, I> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("InodeGuard must never drop.");
    }
}

/// in-memory copy of an inode
pub struct Inode<I: InodeInner> {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    pub inner: SleepLock<I>,
}

pub type Itable<I> = ArrayArena<Inode<I>, NINODE>;

/// A reference counted smart pointer to an `Inode`.
pub type RcInode<I> = ArenaRc<Itable<I>>;

pub struct Tx<'s, FS: FileSystem>
where
    Inode<FS::InodeInner>: ArenaObject,
{
    fs: &'s FS,
}

impl<FS: FileSystem> Drop for Tx<'_, FS>
where
    Inode<FS::InodeInner>: ArenaObject,
{
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Tx must never drop.");
    }
}

impl<FS: FileSystem> Tx<'_, FS>
where
    Inode<FS::InodeInner>: ArenaObject,
{
    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub fn end(self, ctx: &KernelCtx<'_, '_>) {
        unsafe {
            self.fs.tx_end(ctx);
        }
        core::mem::forget(self);
    }
}

pub trait InodeInner: 'static + Unpin + Send + Sized {
    /// Read data from inode.
    ///
    /// `f` takes an offset and a slice as arguments. `f(off, src, ctx)` should copy
    /// the content of `src` to the interval beginning at `off`th byte of the
    /// destination, which the caller of this method knows.
    // This method takes a function as an argument, because writing to kernel
    // memory and user memory are very different from each other. Writing to a
    // consecutive region in kernel memory can be done at once by simple memcpy.
    // However, writing to user memory needs page table accesses since a single
    // consecutive region in user memory may split into several pages in
    // physical memory.
    fn read_internal<
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
    ) -> Result<usize, ()>;
}

pub trait FileSystem: Sized
where
    Inode<Self::InodeInner>: ArenaObject,
{
    type Dirent;
    type InodeInner: InodeInner;

    /// Initializes the file system (loading from the disk).
    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>);

    /// Finds the root inode.
    fn root(self: StrongPin<'_, Self>) -> RcInode<Self::InodeInner>;

    /// Finds inode from the given path.
    fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Self::InodeInner>, ()>;

    /// Create another name(newname) for the file oldname.
    /// Returns Ok(()) on success, Err(()) on error.
    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self::InodeInner>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()>;

    /// Remove a file(filename).
    /// Returns Ok(()) on success, Err(()) on error.
    fn unlink(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()>;

    /// Create an inode with given type.
    /// Returns Ok(created inode, result of given function f) on success, Err(()) on error.
    fn create<F, T>(
        self: StrongPin<'_, Self>,
        path: &Path,
        typ: InodeType,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
        f: F,
    ) -> Result<(RcInode<Self::InodeInner>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self::InodeInner>) -> T;

    /// Open a file; omode indicate read/write.
    /// Returns Ok(file descriptor) on success, Err(()) on error.
    fn open(
        self: StrongPin<'_, Self>,
        path: &Path,
        omode: FcntlFlags,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()>;

    /// Change the current directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn chdir(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self::InodeInner>,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()>;

    /// Begins a transaction.
    ///
    /// Called for each FS system call.
    fn tx_begin(&self, ctx: &KernelCtx<'_, '_>);

    /// Ends a transaction.
    ///
    /// Called at the end of each FS system call.
    ///
    /// # Safety
    ///
    /// `tx_end` should not be called more than `tx_begin`. Also, f system APIs should be called
    /// inside a transaction.
    unsafe fn tx_end(&self, ctx: &KernelCtx<'_, '_>);
}

pub trait FileSystemExt: FileSystem
where
    Inode<Self::InodeInner>: ArenaObject,
{
    /// Begins a transaction.
    fn begin_tx(&self, ctx: &KernelCtx<'_, '_>) -> Tx<'_, Self>;
}

impl<FS: FileSystem> FileSystemExt for FS
where
    Inode<FS::InodeInner>: ArenaObject,
{
    fn begin_tx(&self, ctx: &KernelCtx<'_, '_>) -> Tx<'_, Self> {
        self.tx_begin(ctx);
        Tx { fs: self }
    }
}
