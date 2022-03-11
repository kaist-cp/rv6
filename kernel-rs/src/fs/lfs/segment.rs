use core::ptr;

use array_macro::array;
use static_assertions::const_assert;

use crate::{
    bio::{Buf, BufUnlocked},
    hal::hal,
    param::{BSIZE, SEGSIZE},
    proc::KernelCtx,
};

/// Entries for the in-memory segment summary.
#[allow(dead_code)]
pub enum SegSumEntry {
    Empty,
    Inode {
        inum: u32,
        buf: BufUnlocked,
    },
    /// Data block of an inode.
    DataBlock {
        inum: u32,
        block_no: u32,
        buf: BufUnlocked,
    },
    /// The block that stores the mapping for all indirect data blocks of an inode.
    IndirectMap {
        inum: u32,
        buf: BufUnlocked,
    },
    Imap {
        block_no: u32,
        buf: BufUnlocked,
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
struct DSegSum([DSegSumEntry; SEGSIZE - 1]);

impl SegSumEntry {
    /// Returns a reference to the `BufUnlocked` hold by the `SegSumEntry`.
    fn get_buf(&self) -> Option<&BufUnlocked> {
        match self {
            SegSumEntry::Empty => None,
            SegSumEntry::Inode { inum: _, buf } => Some(&buf),
            SegSumEntry::DataBlock {
                inum: _,
                block_no: _,
                buf,
            } => Some(&buf),
            SegSumEntry::IndirectMap { inum: _, buf } => Some(&buf),
            SegSumEntry::Imap { block_no: _, buf } => Some(&buf),
        }
    }
}

impl DSegSum {
    fn new(segment_summary: &[SegSumEntry; SEGSIZE - 1]) -> Self {
        Self(array![x => match segment_summary[x] {
                SegSumEntry::Empty => DSegSumEntry { block_type: 0, inum: 0, block_no: 0 },
                SegSumEntry::Inode { inum, .. } => DSegSumEntry { block_type: 1, inum, block_no: 0 },
                SegSumEntry::DataBlock { inum, block_no, .. } => DSegSumEntry { block_type: 2, inum, block_no },
                SegSumEntry::IndirectMap { inum, .. } => DSegSumEntry { block_type: 3, inum, block_no: 0 },
                SegSumEntry::Imap { block_no, .. } => DSegSumEntry { block_type: 4, inum: 0, block_no },
        }; SEGSIZE - 1])
    }
}

/// In-memory segment.
/// The segment is the unit of sequential disk writes.
///
/// Any write operations to the disk must be done through the `Segment`'s methods.
/// That is, when you want to write something new to the disk (ex: create a new indoe)
/// or update something already on the disk (ex: update an inode/inode data block/imap),
/// you should request for a `Buf` to the `Segment` and write on it.
///
/// # Note
///
/// The `Segment` does not always provide an empty data block to the outside.
/// When requesting for a new block to be used to update an inode/inode data block/imap,
/// if a block for it was already requested before and is still on the segment (i.e. not committed yet),
/// the `Segment` just returns the `Buf` of that block instead of an empty one.
/// In this case, you just need to update the `Buf` for only the parts that actually changed.
// We only actually hold the segment summary in memory.
// When we flush the segment, we create a DSegSum (on-disk segment summary block) and write it together with
// the in-memory data (inode from `Itable`, inode data block from `Buf`, and inode map from `Imap`) for each
// segment block to the disk.
pub struct Segment {
    dev_no: u32,

    /// The segment number of this segment.
    segment_no: u32,

    /// Segment summary. Indicates info for each block that should be in the segment.
    // TODO: Use ArrayVec instead?
    segment_summary: [SegSumEntry; SEGSIZE - 1],

    /// Current offset of the segment. Must flush when `offset == SEGSIZE - 1`.
    offset: usize,
}

impl const Default for Segment {
    fn default() -> Self {
        Self {
            dev_no: 0,
            segment_no: 0,
            segment_summary: array![_ => SegSumEntry::Empty; SEGSIZE - 1],
            offset: 0,
            // imap_block_no: 0,
        }
    }
}

// TODO: Generalize methods of `Segment`.
impl Segment {
    #[allow(dead_code)]
    // TODO: Load from a non-empty segment instead?
    pub const fn new(dev_no: u32, segment_no: u32) -> Self {
        Self {
            dev_no,
            segment_no,
            segment_summary: array![_ => SegSumEntry::Empty; SEGSIZE - 1],
            offset: 0,
        }
    }

    /// Returns the disk block number for the `seg_block_no`th block on this segment.
    fn get_disk_block_no(&self, seg_block_no: usize, ctx: &KernelCtx<'_, '_>) -> u32 {
        ctx.kernel()
            .fs()
            .superblock()
            .seg_to_disk_block_no(self.segment_no, seg_block_no as u32)
    }

    fn read_segment_block(&self, seg_block_no: usize, ctx: &KernelCtx<'_, '_>) -> Buf {
        hal()
            .disk()
            .read(self.dev_no, self.get_disk_block_no(seg_block_no, ctx), ctx)
    }

    /// Returns true if the segment has no more remaining blocks.
    /// You should commit the segment immediately after this.
    pub fn is_full(&self) -> bool {
        self.offset == SEGSIZE - 1
    }

    /// Returns the number of remaining blocks on the segment.
    pub fn remaining(&self) -> usize {
        SEGSIZE - 1 - self.offset
    }

    /// Provides an empty block on the segment to be used to store a new inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    ///
    /// # Note
    ///
    /// Allocating an inode is done as following.
    /// 1. Traverse the `Imap` to find an unused inum.
    /// 2. Allocate an `RcInode` from the `Itable` using the dev_no, inum.
    /// 3. (this method) Use the `RcInode` to allocate an inode block and get the `Buf`.
    /// 4. Write the initial `Dinode` on the `Buf`.
    pub fn add_new_inode_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            let mut buf = self.read_segment_block(self.offset + 1, ctx);
            buf.deref_inner_mut().data.fill(0);
            buf.deref_inner_mut().valid = true;
            let buf = buf.unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::Inode {
                inum,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides an empty block on the segment to be used to store a new data block of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    pub fn add_new_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            let mut buf = self.read_segment_block(self.offset + 1, ctx);
            buf.deref_inner_mut().data.fill(0);
            buf.deref_inner_mut().valid = true;
            let buf = buf.unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::DataBlock {
                inum,
                block_no,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides an empty block on the segment to be used to store the new indirect map of an inode.
    /// Use this if and only if the inode does not already have one.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    pub fn add_new_indirect_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            let mut buf = self.read_segment_block(self.offset + 1, ctx);
            buf.deref_inner_mut().data.fill(0);
            buf.deref_inner_mut().valid = true;
            let buf = buf.unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::IndirectMap {
                inum,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides a block on the segment to be used to store the updated inode.
    /// If the inode is not already on the segment, allocates an empty block on the segment for it.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    ///
    /// Run this every time an inode gets updated.
    ///
    /// # Note
    ///
    /// Use this only when updating an inode. For allocating a new inode, use `Segment::add_new_node_block` instead.
    pub fn get_or_add_updated_inode_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        // TODO: Maybe more efficient if we make the `Inode` bookmark this.
        for i in (0..self.offset).rev() {
            if let SegSumEntry::Inode { inum: inum2, buf } = &self.segment_summary[i] {
                if inum == *inum2 {
                    return Some((buf.clone().lock(ctx), self.get_disk_block_no(i + 1, ctx)));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            let buf = self.read_segment_block(self.offset + 1, ctx).unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::Inode {
                inum,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides a block on the segment to be used to store the new/updated data block of an inode.
    /// If the inode's `block_no`th data block is not already on the segment, allocates an empty block on the segment for it.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    ///
    /// Whenever a data block gets updated, run this and write the new data at the returned `Buf`.
    pub fn get_or_add_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        for i in (0..self.offset).rev() {
            if let SegSumEntry::DataBlock {
                inum: inum2,
                block_no: block_no2,
                buf,
            } = &self.segment_summary[i]
            {
                if inum == *inum2 && block_no == *block_no2 {
                    return Some((buf.clone().lock(ctx), self.get_disk_block_no(i + 1, ctx)));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            // TODO: We unlock a buffer right after locking it. This may be inefficient.
            let buf = self.read_segment_block(self.offset + 1, ctx).unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::DataBlock {
                inum,
                block_no,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides a block on the segment to be used to store the new/updated indirect mapping block of an inode.
    /// If the inode's indirect mapping block is not already on the segment, allocates an empty block on the segment for it.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    ///
    /// Whenever an inode's indirect data block's address changes, run this and update the mapping.
    pub fn get_or_add_indirect_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        for i in (0..self.offset).rev() {
            if let SegSumEntry::IndirectMap { inum: inum2, buf } = &self.segment_summary[i] {
                if inum == *inum2 {
                    return Some((buf.clone().lock(ctx), self.get_disk_block_no(i + 1, ctx)));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            // TODO: We unlock a buffer right after locking it. This may be inefficient.
            let buf = self.read_segment_block(self.offset + 1, ctx).unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::IndirectMap {
                inum,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides an empty space on the segment to be used to store the updated imap.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// If the `block_no`th imap block is not already on the segment, allocates an empty block on the segment for it.
    ///
    /// Whenever the imap gets updated, run this with the proper block_no.
    pub fn get_or_add_imap_block(
        &mut self,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        // TODO: We could just bookmark at `Segment` instead.
        for i in (0..self.offset).rev() {
            if let SegSumEntry::Imap {
                block_no: block_no2,
                buf,
            } = &self.segment_summary[i]
            {
                if block_no == *block_no2 {
                    return Some((buf.clone().lock(ctx), self.get_disk_block_no(i + 1, ctx)));
                }
            }
        }
        // Try to push at the back of the segment.
        if self.is_full() {
            None
        } else {
            // Append segment.
            // TODO: We unlock a buffer right after locking it. This may be inefficient.
            let buf = self.read_segment_block(self.offset + 1, ctx).unlock(ctx);
            self.segment_summary[self.offset] = SegSumEntry::Imap {
                block_no,
                buf: buf.clone(),
            };
            self.offset += 1;
            Some((buf.lock(ctx), self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Commits the segment to the disk. Updates the checkpoint region of the disk if needed.
    /// Run this when the segment is full or right before shutdowns.
    pub fn commit(&mut self, ctx: &KernelCtx<'_, '_>) {
        const_assert!(core::mem::size_of::<DSegSum>() <= BSIZE);

        // Write the segment summary to the disk.
        let mut bp = self.read_segment_block(0, ctx);
        let ssp = bp.deref_inner_mut().data.as_mut_ptr() as *mut DSegSum;
        unsafe { ptr::write(ssp, DSegSum::new(&self.segment_summary)) };
        bp.free(ctx);

        // Write each segment block to the disk.
        // TODO: Check the virtio spec for a way for faster sequential disk write.
        for i in 0..self.offset {
            let entry = &self.segment_summary[i];
            if let Some(buf) = entry.get_buf() {
                let mut buf = buf.clone().lock(ctx);
                hal().disk().write(&mut buf, ctx);
                buf.free(ctx);
            }
        }

        self.segment_no = ctx.kernel().fs().get_next_seg_no(Some(self.segment_no));
        self.segment_summary = array![_ => SegSumEntry::Empty; SEGSIZE - 1];
        self.offset = 0;

        // TODO: Update the on-disk checkpoint if needed.
    }
}
