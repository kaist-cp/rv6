use core::mem;

use static_assertions::const_assert;

use crate::{
    bio::{Buf, BufData},
    param::{BSIZE, SEGSIZE},
};

const FSMAGIC: u32 = 0x10203040;

// Disk layout:
// [ boot block | super block | checkpoint1  | checkpoint2 |
//                                          inode map, inode blocks, and data blocks ]
//
// mklfs computes the super block and builds an initial file system. The
// super block describes the disk layout:
#[repr(C)]
#[derive(Clone)]
pub struct Superblock {
    /// Must be FSMAGIC
    magic: u32,

    /// Size of file system image (blocks)
    size: u32,

    /// Number of data blocks
    nblocks: u32,

    /// Number of segments
    nsegments: u32,

    /// Number of inodes
    ninodes: u32,

    // Block number of first checkpoint block
    checkpoint1: u32,

    // Block number of second checkpoint block
    checkpoint2: u32,

    // Block number of first segment
    segstart: u32,
}

impl<'s> TryFrom<&'s BufData> for &'s Superblock {
    type Error = &'static str;

    fn try_from(b: &'s BufData) -> Result<Self, Self::Error> {
        const_assert!(mem::size_of::<Superblock>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<Superblock>() == 0);

        // Disk content uses intel byte order.
        let magic = u32::from_le_bytes(b[..mem::size_of::<u32>()].try_into().unwrap());
        if magic == FSMAGIC {
            // SAFETY:
            // * buf.data is larger than Superblock
            // * buf.data is aligned properly.
            // * Superblock contains only u32's, so does not have any requirements.
            Ok(unsafe { &*(b.as_ptr() as *const Superblock) })
        } else {
            Err("invalid file system")
        }
    }
}

impl Superblock {
    /// Read the super block.
    pub fn new(buf: &Buf) -> Self {
        let sb: &Superblock = buf.data().try_into().unwrap();
        sb.clone()
    }

    pub fn ninodes(&self) -> u32 {
        self.ninodes
    }

    pub fn nsegments(&self) -> u32 {
        self.nsegments
    }

    /// Translates (segment number, segment block number) -> disk block number.
    pub fn seg_to_disk_block_no(&self, seg_no: u32, seg_block_no: u32) -> u32 {
        seg_no
            .wrapping_mul(SEGSIZE as u32)
            .wrapping_add(seg_block_no + self.segstart)
    }

    /// Translates disk block number -> (segment number, segment block number)
    #[allow(dead_code)]
    pub fn disk_to_seg_block_no(&self, disk_block_no: u32) -> (u32, u32) {
        (
            (disk_block_no - self.segstart) / (SEGSIZE as u32),
            (disk_block_no - self.segstart) % (SEGSIZE as u32),
        )
    }

    /// Returns the starting block number of each checkpoint region.
    pub fn get_chkpt_block_no(&self) -> (u32, u32) {
        (self.checkpoint1, self.checkpoint2)
    }
}
