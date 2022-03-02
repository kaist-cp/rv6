use core::{mem, ptr};

use static_assertions::const_assert;

use crate::{
    bio::{Buf, BufData},
    param::{BSIZE, SEGSIZE},
};

const FSMAGIC: u32 = 0x10203040;

// TODO: re-define imap to set the fields of superblock
// CheckpointRegion should be made as a new struct to handle imap locations

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

    /// Current segment
    pub cur_segment: u32,

    /// Location of imap
    pub imap_location: u32,

    /// Checkpoint region
    /// - allocating recent two checkpoint regions for crash recovery
    pub checkpoint_region: (u32, u32),
}

/// Bitmap bits per block
// pub const BPB: usize = BSIZE * 8;

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

    /// Translates (segment number, segment block number) -> disk block number.
    pub fn seg_to_disk_block_no(&self, seg_no: u32, seg_block_no: u32) -> u32 {
        // TODO: Fix this after deciding the disk layout.
        seg_no
            .wrapping_mul(SEGSIZE as u32)
            .wrapping_add(seg_block_no)
    }

    /// Translates disk block number -> (segment number, segment block number)
    #[allow(dead_code)]
    pub fn disk_to_seg_block_no(&self, disk_block_no: u32) -> (u32, u32) {
        // TODO: Fix this after deciding the disk layout.
        (
            disk_block_no / (SEGSIZE as u32),
            disk_block_no % (SEGSIZE as u32),
        )
    }
}
