use core::ops::Deref;

use bitflags::bitflags;
use cstr_core::CStr;
pub use path::{FileName, Path};
pub use stat::Stat;
pub use ufs::{InodeInner as UfsInodeInner, Ufs};

use crate::{lock::Sleeplock, proc::KernelCtx};

mod path;
mod stat;
mod ufs;

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

/// InodeGuard implies that `Sleeplock<InodeInner>` is held by current thread.
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
pub struct InodeGuard<'a, I> {
    pub inode: &'a Inode<I>,
}

impl<I> Deref for InodeGuard<'_, I> {
    type Target = Inode<I>;

    fn deref(&self) -> &Self::Target {
        self.inode
    }
}

impl<I> InodeGuard<'_, I> {
    pub fn deref_inner(&self) -> &I {
        // SAFETY: self.inner is locked.
        unsafe { &*self.inner.get_mut_raw() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut I {
        // SAFETY: self.inner is locked and &mut self is exclusive.
        unsafe { &mut *self.inner.get_mut_raw() }
    }
}

/// Unlock and put the given inode.
impl<I> Drop for InodeGuard<'_, I> {
    fn drop(&mut self) {
        // SAFETY: self will be dropped.
        unsafe { self.inner.unlock() };
    }
}

/// in-memory copy of an inode
pub struct Inode<I> {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    pub inner: Sleeplock<I>,
}

pub trait FileSystem {
    type Dirent;
    type Inode;
    type Tx<'s>;

    /// Initializes the file system (loading from the disk).
    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>);

    /// Called for each FS system call.
    fn begin_tx(&self) -> Self::Tx<'_>;

    /// Create another name(newname) for the file oldname.
    /// Returns Ok(()) on success, Err(()) on error.
    fn link(
        &self,
        oldname: &CStr,
        newname: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()>;

    /// Remove a file(filename).
    /// Returns Ok(()) on success, Err(()) on error.
    fn unlink(&self, filename: &CStr, tx: &Self::Tx<'_>, ctx: &KernelCtx<'_, '_>)
        -> Result<(), ()>;

    /// Open a file; omode indicate read/write.
    /// Returns Ok(file descriptor) on success, Err(()) on error.
    fn open(
        &self,
        name: &Path,
        omode: FcntlFlags,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()>;

    /// Change the current directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn chdir(
        &self,
        dirname: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()>;
}
