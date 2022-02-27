use core::ptr;

use array_macro::array;
use static_assertions::const_assert;

use super::{Dinode, InodeType, Lfs, RcInode};
use crate::{
    bio::{Buf, BufUnlocked},
    fs::DInodeType,
    hal::hal,
    param::{BSIZE, NBLOCK},
    proc::KernelCtx,
};

/// Entries for the in-memory segment summary.
#[allow(dead_code)]
pub enum SegSumEntry {
    Empty,
    Inode {
        inode: RcInode<Lfs>,
    },
    DataBlock {
        inum: u32,
        block_no: u32,
        buf: BufUnlocked,
    },
    Imap {
        block_no: u32,
    },
}

/// On-disk segment summary entry structure.
#[repr(C)]
struct DSegSumEntry {
    /// 0: empty, 1: inode, 2: data block, 3: imap block
    block_type: u32,
    inum: u32,
    block_no: u32,
}

/// On-disk segment summary structure.
#[repr(C)]
struct DSegSum([DSegSumEntry; NBLOCK - 1]);

impl DSegSum {
    fn new(segment_summary: &[SegSumEntry; NBLOCK - 1]) -> Self {
        Self(array![x => match segment_summary[x] {
                SegSumEntry::Empty => DSegSumEntry { block_type: 0, inum: 0, block_no: 0 },
                SegSumEntry::Inode { inode } => DSegSumEntry { block_type: 1, inum: inode.inum, block_no: 0 },
                SegSumEntry::DataBlock { inum, block_no, buf } => DSegSumEntry { block_type: 2, inum, block_no },
                SegSumEntry::Imap { block_no } => DSegSumEntry { block_type: 3, inum: 0, block_no },
        }; NBLOCK - 1])
    }
}

/// In-memory segment.
/// The segment is the unit of sequential disk writes.
///
/// # Note
///
/// We only actually hold the segment summary in memory.
/// When we flush the segment, we create a DSegSum (on-disk segment summary block) and write it together with
/// the in-memory data (inode from `Itable`, inode data block from `Buf`, and inode map from `Imap`) for each
/// segment block to the disk.
pub struct Segment {
    dev_no: u32,

    /// The segment number of this segment.
    segment_no: u32,

    /// Segment summary. Indicates info for each block that should be in the segment.
    // TODO: Use ArrayVec instead?
    segment_summary: [SegSumEntry; NBLOCK - 1],

    /// Current offset of the segment. Must flush when `offset == NBLOCK - 1`.
    offset: usize,
    /* A mapping of (imap block number) -> (its location on the segment).
     * imap_block_no: [u32; NENTRY], */
}

impl const Default for Segment {
    fn default() -> Self {
        Self {
            dev_no: 0,
            segment_no: 0,
            segment_summary: array![_ => SegSumEntry::Empty; NBLOCK - 1],
            offset: 0,
            // imap_block_no: 0,
        }
    }
}

impl Segment {
    pub const fn new(
        dev_no: u32,
        segment_no: u32,
        segment_summary: [SegSumEntry; NBLOCK - 1],
        offset: usize,
    ) -> Self {
        Self {
            dev_no,
            segment_no,
            segment_summary,
            offset,
            // imap_block_no,
        }
    }

    fn read_segment_block(&self, seg_block_no: u32, ctx: &KernelCtx<'_, '_>) -> Buf {
        // TODO: Fix this after deciding the disk layout.
        // TODO: Check casting errors?
        let block_no = self
            .segment_no
            .wrapping_mul(NBLOCK as u32)
            .wrapping_add(seg_block_no);
        hal().disk().read(self.dev_no, block_no, ctx)
    }

    pub fn is_full(&self) -> bool {
        self.offset == NBLOCK - 1
    }

    /// Provides an empty space on the segment to be used to store the new `inode`.
    /// If succeeds, returns the segment number, segment block number, and a `Buf` of the segment block.
    ///
    /// # Note
    ///
    /// Allocating an inode is done as following.
    /// 1. Traverse the `Imap` to find an unused inum.
    /// 2. Allocate an `RcInode` from the `Itable` using the dev_no, inum.
    /// 3. (this method) Use the `RcInode` to allocate an inode block and get the `Buf`.
    /// 4. Write the initial `Dinode` on the `Buf`.
    pub fn alloc_inode_block(
        &mut self,
        inode: &RcInode<Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(u32, u32, Buf)> {
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            self.segment_summary[self.offset] = SegSumEntry::Inode {
                inode: inode.clone(),
            };
            self.offset += 1;
            let buf = self.read_segment_block(self.offset as u32, ctx);
            Some((self.segment_no, self.offset as u32, buf))
        }
    }

    /// Appends the inode at the back of the segment after cloning the given `RcInode` if it is not already on the segment.
    /// Returns the location as (segment number, segment block number) if succeeded. Otherwise, returns `None`.
    /// Run this every time an inode gets updated.
    pub fn push_back_inode_block(&mut self, inode: &RcInode<Lfs>) -> Option<(u32, u32)> {
        // Check if the block already exists.
        // TODO: Maybe more efficient if we make the `Inode` bookmark this.
        for i in 0..self.offset {
            if let SegSumEntry::Inode { inode: inode2 } = self.segment_summary[i] {
                if inode.inum == inode2.inum {
                    return Some((self.segment_no, (i + 1) as u32));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            self.segment_summary[self.offset] = SegSumEntry::Inode {
                inode: inode.clone(),
            };
            self.offset += 1;
            Some((self.segment_no, self.offset as u32))
        }
    }

    /// Provides an empty space on the segment to be used to store the new data block for an inode.
    /// If succeeds, returns the segment number, segment block number, and a `Buf` of the segment block.
    /// Whenever a data block gets updated, run this and write the new data at the returned `Buf`.
    pub fn alloc_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(u32, u32, Buf)> {
        // Check if the block already exists.
        for i in 0..self.offset {
            if let SegSumEntry::DataBlock {
                inum: inum2,
                block_no: block_no2,
                buf,
            } = self.segment_summary[i]
            {
                if inum == inum2 && block_no == block_no2 {
                    return Some((self.segment_no, (i + 1) as u32, buf.clone().lock(ctx)));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            // TODO: We unlock a buffer right after locking it. This may be inefficient.
            let buf = self
                .read_segment_block((self.offset + 1) as u32, ctx)
                .unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::DataBlock {
                inum,
                block_no,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((self.segment_no, self.offset as u32, buf.lock(ctx)))
        }
    }

    /// Appends the inode map at the back of the segment if it is not already on the segment.
    /// Returns the location as (segment number, segment block number) if succeeded. Otherwise, returns `None`.
    /// Run this whenever the inode map get updated.
    pub fn append_inode_map_block(&mut self, block_no: u32) -> Option<(u32, u32)> {
        // TODO: Check if it's already on the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            self.segment_summary[self.offset] = SegSumEntry::Imap { block_no };
            self.offset += 1;
            Some((self.segment_no, self.offset as u32))
        }
    }

    /// Commits the segment to the disk. Updates the checkpoint region of the disk if needed.
    /// Run this when the segment is full or right before shutdown.
    pub fn commit(&mut self, ctx: &KernelCtx<'_, '_>) {
        const_assert!(core::mem::size_of::<DSegSum>() <= BSIZE);

        // Write the segment summary to the disk.
        let bp = self.read_segment_block(0, ctx);
        let ssp = bp.deref_inner().data.as_mut_ptr() as *mut DSegSum;
        unsafe { ptr::write(ssp, DSegSum::new(&self.segment_summary)) };

        // Write each segment block to the disk.
        // TODO: Check the virtio spec for a way for faster sequential disk write.
        for i in 0..self.offset {
            let entry = &self.segment_summary[i];
            match entry {
                SegSumEntry::Empty => (),
                SegSumEntry::Inode { inode } => {
                    let guard = inode.lock(ctx);
                    if !guard.deref_inner().valid || guard.deref_inner().nlink != 0 {
                        // Write to buffer.
                        let bp = self.read_segment_block((i + 1) as u32, ctx);
                        let dip = unsafe {
                            &mut *(bp.deref_inner_mut().data.as_mut_ptr() as *mut Dinode)
                        };
                        let inner = guard.deref_inner();
                        match inner.typ {
                            InodeType::Device { major, minor } => {
                                dip.typ = DInodeType::Device;
                                dip.major = major;
                                dip.minor = minor;
                            }
                            InodeType::None => {
                                dip.typ = DInodeType::None;
                                dip.major = 0;
                                dip.minor = 0;
                            }
                            InodeType::Dir => {
                                dip.typ = DInodeType::Dir;
                                dip.major = 0;
                                dip.minor = 0;
                            }
                            InodeType::File => {
                                dip.typ = DInodeType::File;
                                dip.major = 0;
                                dip.minor = 0;
                            }
                        }

                        (*dip).nlink = inner.nlink;
                        (*dip).size = inner.size;
                        for (d, s) in (*dip).addr_direct.iter_mut().zip(&inner.addr_direct) {
                            *d = *s;
                        }
                        (*dip).addr_indirect = inner.addr_indirect;

                        // Now write to disk.
                        hal().disk().write(&mut bp, ctx)
                    }
                }
                SegSumEntry::DataBlock {
                    inum,
                    block_no,
                    buf,
                } => hal().disk().write(&mut buf.lock(ctx), ctx),
                SegSumEntry::Imap { block_no } => (), //TODO: Write the imap to disk
            };
        }

        // TODO: The `Lfs` should provide a new segment.
        self.segment_no += 1;
        self.segment_summary = array![_ => SegSumEntry::Empty; NBLOCK - 1];
        self.offset = 0;

        // TODO: Update the on-disk checkpoint if needed.
    }
}
