use core::{mem, ptr};

use static_assertions::const_assert;

use super::Imap;
use crate::{
    bio::{Buf, BufData},
    hal::hal,
    param::{BSIZE, IMAPSIZE, SEGSIZE, SEGTABLESIZE},
    proc::KernelCtx,
};

const FSMAGIC: u32 = 0x10203040;

// Disk layout:
// [ boot block | super block | checkpoint1  | checkpoint2 |
//                                          inode map, inode blocks, and data blocks ]
//
// mklfs computes the super block and builds an initial file system. The
// super block describes the disk layout:
#[repr(C)]
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

pub type SegTable = [u8; SEGTABLESIZE];

/// On-disk checkpoint structure.
#[repr(C)]
struct Checkpoint {
    imap: [u32; IMAPSIZE],
    segtable: SegTable,
    timestamp: u32,
}

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

    /// Loads the latest checkpoint from the disk.
    pub fn load_checkpoint(
        &self,
        dev_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> ([u8; SEGTABLESIZE], usize, Imap) {
        let buf1 = hal().disk().read(dev_no, self.checkpoint1, ctx);
        let chkpt1 = unsafe { &*(buf1.deref_inner().data.as_ptr() as *const Checkpoint) };
        let buf2 = hal().disk().read(dev_no, self.checkpoint2, ctx);
        let chkpt2 = unsafe { &*(buf2.deref_inner().data.as_ptr() as *const Checkpoint) };

        let (chkpt, chkpt_no) = if chkpt1.timestamp > chkpt2.timestamp {
            (chkpt1, 1)
        } else {
            (chkpt2, 2)
        };

        let result = (
            chkpt.segtable.clone(),
            chkpt_no,
            Imap::new(dev_no, self.ninodes() as usize, chkpt.imap.clone()),
        );
        buf1.free(ctx);
        buf2.free(ctx);
        result
    }

    #[allow(dead_code)]
    pub fn write_checkpoint(
        &self,
        _dev_no: u32,
        _segtable: &SegTable,
        _imap: &Imap,
        _chkpt_no: usize,
        _ctx: &KernelCtx<'_, '_>,
    ) {
        todo!()
    }
}
