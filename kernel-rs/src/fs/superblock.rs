use core::{mem, ptr};

use crate::{bio::Buf, param::BSIZE};

use super::Dinode;

const FSMAGIC: u32 = 0x10203040;

/// Disk layout:
/// [ boot block | super block | log | inode blocks |
///                                          free bit map | data blocks]
///
/// mkfs computes the super block and builds an initial file system. The
/// super block describes the disk layout:
#[derive(Copy, Clone)]
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
pub const IPB: usize = BSIZE.wrapping_div(mem::size_of::<Dinode>());

/// Bitmap bits per block
pub const BPB: u32 = BSIZE.wrapping_mul(8) as u32;

impl Superblock {
    /// Read the super block.
    pub unsafe fn new(buf: &Buf<'static>) -> Self {
        let result = ptr::read(buf.deref_inner().data.as_ptr() as *const Superblock);
        assert_eq!(result.magic, FSMAGIC, "invalid file system");
        result
    }

    /// Block containing inode i
    pub const fn iblock(self, i: u32) -> u32 {
        i.wrapping_div(IPB as u32).wrapping_add(self.inodestart)
    }

    /// Block of free map containing bit for block b
    pub const fn bblock(self, b: u32) -> u32 {
        b.wrapping_div(BPB).wrapping_add(self.bmapstart)
    }
}
