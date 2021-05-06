mod path;
mod stat;

mod ufs;

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
}
