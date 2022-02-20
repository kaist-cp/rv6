use core::{mem, ptr};

use static_assertions::const_assert;

use super::inode::Dinode;
use crate::{
    bio::{Buf, BufData},
    param::BSIZE,
};

const FSMAGIC: u32 = 0x10203040;

/// Disk layout:
/// [ boot block | super block | data block | inode | ... | inode map ]
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

    /// Number of free blocks
    pub nfree_blocks: u32,

    // TODO: Okay to move to `Segment::segment_no`?
    /// Current segment
    // pub cur_segment: u32,

    // TODO: Unnecessary if use current time (or something similar)?
    /// Checkpoint region
    pub checkpoint_region: u32,
}

/// Inodes per block.
#[allow(dead_code)]
pub const IPB: usize = BSIZE / mem::size_of::<Dinode>();

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
}
