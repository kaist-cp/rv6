mod path;
mod stat;

pub mod ufs;

pub use path::{FileName, Path};
pub use stat::Stat;

pub use crate::proc::KernelCtx;

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
    type Tx<'s>: Tx<'s>;

    /// Initializes the file system (loading from the disk).
    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>);

    /// Called for each FS system call.
    fn begin_tx(&self) -> Self::Tx<'_>;
}

pub trait Tx<'s> {}
