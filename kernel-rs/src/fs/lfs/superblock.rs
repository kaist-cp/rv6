use core::{mem, ptr};

use static_assertions::const_assert;

use super::inode::Dinode;
use crate::{
    bio::{Buf, BufData},
    param::BSIZE,
};

const FSMAGIC: u32 = 0x10203040;

/// Disk layout:
/// [ boot block | super block | log | inode blocks |
///                                          free bit map | data blocks]
///
/// mkfs computes the super block and builds an initial file system. The
/// super block describes the disk layout:
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Superblock {
    /// Must be FSMAGIC
    magic: u32,

    /// Size of file system image (blocks)
    pub size: u32,

    /// Number of data blocks
    nblocks: u32,

    /// Number of inodes
    pub ninodes: u32,

    /// Number of log blocks
    pub nlog: u32,

    /// Block number of first log block
    pub logstart: u32,

    /// Block number of first inode block
    pub inodestart: u32,

    /// Block number of first free map block
    pub bmapstart: u32,
}

/// Inodes per block.
pub const IPB: usize = BSIZE / mem::size_of::<Dinode>();

/// Bitmap bits per block
pub const BPB: usize = BSIZE * 8;

impl Superblock {
    /// Read the super block.
    pub fn new(buf: &Buf) -> Self {
        const_assert!(mem::size_of::<Superblock>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<Superblock>() == 0);
        // SAFETY:
        // * buf.data is larger than Superblock
        // * buf.data is aligned properly.
        // * Superblock contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let result = unsafe { ptr::read(buf.deref_inner().data.as_ptr() as *const Superblock) };
        assert_eq!(result.magic, FSMAGIC, "invalid file system");
        result
    }

    /// Block containing inode i
    pub const fn iblock(self, i: u32) -> u32 {
        i / IPB as u32 + self.inodestart
    }

    /// Block of free map containing bit for block b
    pub const fn bblock(self, b: u32) -> u32 {
        b / BPB as u32 + self.bmapstart
    }
}
