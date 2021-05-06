mod path;
mod stat;

mod ufs;

use bitflags::bitflags;
use cstr_core::CStr;
pub use path::{FileName, Path};
pub use stat::Stat;
// TODO: UfsInodeGuard must be hidden.
pub use ufs::{InodeGuard as UfsInodeGuard, Ufs};

use crate::proc::KernelCtx;

#[derive(Copy, Clone, PartialEq, Debug)]
#[repr(i16)]
pub enum InodeType {
    None,
    Dir,
    File,
    Device { major: u16, minor: u16 },
}

bitflags! {
    pub struct FcntlFlags: i32 {
        const O_RDONLY = 0;
        const O_WRONLY = 0x1;
        const O_RDWR = 0x2;
        const O_CREATE = 0x200;
        const O_TRUNC = 0x400;
    }
}

pub trait FileSystem {
    type Dirent;
    type Inode;
    type InodeGuard<'s>;
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
