//! In-memory segment.
//! Any kernel write operations to the disk must be done through this.
//!
//! # How to write something to the disk in the `Lfs`
//!
//! Any kernel write operations to the disk must be done using the `Segment`'s methods in `Lfs`.
//! That is, when you want to write something new to the disk (ex: create a new inode)
//! or update something already on the disk (ex: update an inode/inode data block/imap),
//! you should
//! 1. lock the `Segment`,
//! 2. request the `Segment` for a `Buf`,
//! 3. write on the `Buf`,
//! 4. commit the `Segment` if it's full, and
//! 5. release all locks.
//!
//! # `add_new_*_block` vs `get_or_add_*_block`
//!
//! The `Segment` has two types of methods that provides a `(Buf, u32)` pair to the outside.
//! * `add_new_*_block` methods : Always use these methods when allocating a **new**
//!   inode/inode data block/inode indirect block. These methods always allocate a new zeroed disk block.
//! * `get_or_add_*_block` methods : Always use these methods when updating a **previous**
//!   inode/inode data block/inode indirect block/imap block. These methods may allocate a new zeroed disk block or
//!   just return a `Buf` to a previously allocated block if it is still on the segment.
//!     * Using this method lets us reduce the cost to copy the content from an old buf to a new one every time
//!       we write something to the disk. However, using this method may cause a problem if this is the first
//!       time we allocate a disk block for the inode/inode data block/inode indirect map.
//!       If an inode with the same inum was previously finalized but has some of its blocks still on the segment,
//!       then this method may return a one of these instead of a zeroed one. You must use these methods only for updates.
//!
//! # Updating the `Inode` or `Imap`
//!
//! The `Segment`'s methods does not update the `Imap` by itself. Instead, everytime you use the previously
//! mentioned methods, it returns a `Buf` and a `u32` which holds the disk block number of it.
//! You should manually update the `Inode`'s `addr_direct`/`addr_indirect` field or `Imap`'s mapping
//! using the returned disk block number.
//!
//! # Lock order
//!
//! When acquiring the lock on the `Segment`, `Imap`, or `Buf` at the same time, it must always done in the order of
//! `Segment` -> `Imap` -> `Buf`. Otherwise, you may encounter a deadlock.

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
    /// Inode.
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
    /// Imap.
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
    /// Returns a reference to the `BufUnlocked` owned by the `SegSumEntry`.
    fn get_buf(&self) -> Option<&BufUnlocked> {
        match self {
            SegSumEntry::Empty => None,
            SegSumEntry::Inode { inum: _, buf } => Some(buf),
            SegSumEntry::DataBlock {
                inum: _,
                block_no: _,
                buf,
            } => Some(buf),
            SegSumEntry::IndirectMap { inum: _, buf } => Some(buf),
            SegSumEntry::Imap { block_no: _, buf } => Some(buf),
        }
    }
}

impl DSegSum {
    /// Creates an on-disk `DSegSum` type from the given `SegSumEntry` array.
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
/// Any kernel write operations to the disk must be done through the `Segment`'s methods.
///
/// See the module documentation for details.
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
        }
    }
}

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

    /// Reads the `seg_block_no`th block of this segment, starting from 0 to `SEGSIZE - 1`.
    fn read_segment_block(&self, seg_block_no: usize, ctx: &KernelCtx<'_, '_>) -> Buf {
        hal()
            .disk()
            .read(self.dev_no, self.get_disk_block_no(seg_block_no, ctx), ctx)
    }

    /// Returns true if the segment has no more remaining blocks.
    /// You should commit the segment if the segment is full.
    pub fn is_full(&self) -> bool {
        self.offset == SEGSIZE - 1
    }

    /// Returns the number of remaining blocks on the segment.
    /// You should commit the segment if the segment has less blocks than you need.
    pub fn remaining(&self) -> usize {
        SEGSIZE - 1 - self.offset
    }

    /// Allocates a new zeroed block on the segment and creates a `SegSumEntry` for it using `f`.
    /// Does not care if a block for the same inum/inode block number/imap block number already exists on the segment.
    /// By adding a new block, the previous one will no longer be returned by `Segment`'s methods anyway.
    fn add_new_block<F: FnOnce(BufUnlocked) -> SegSumEntry>(
        &mut self,
        f: F,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        if self.is_full() {
            None
        } else {
            // Append segment with a new zeroed block.
            let mut buf = self.read_segment_block(self.offset + 1, ctx);
            buf.deref_inner_mut().data.fill(0);
            buf.deref_inner_mut().valid = true;
            self.segment_summary[self.offset] = f(buf.create_unlocked());
            self.offset += 1;
            Some((buf, self.get_disk_block_no(self.offset, ctx)))
        }
    }

    /// Provides a new zeroed block on the segment to be used to store a new inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// Always use this if this is the first time we allocate a block for the inode.
    pub fn add_new_inode_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.add_new_block(|buf| SegSumEntry::Inode { inum, buf }, ctx)
    }

    /// Provides a new zeroed block on the segment to be used to store a new data block of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// /// Always use this if this is the first time we allocate the `block_no`th data block for the inode.
    pub fn add_new_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.add_new_block(
            |buf| {
                SegSumEntry::DataBlock {
                    inum,
                    block_no,
                    buf,
                }
            },
            ctx,
        )
    }

    /// Provides a new zeroed block on the segment to be used to store the new indirect map of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// Always use this if this is the first time we allocate the indirect block for the inode.
    pub fn add_new_indirect_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.add_new_block(|buf| SegSumEntry::IndirectMap { inum, buf }, ctx)
    }

    /// Uses `c` to check if the block is already on the segment, and returns a `Buf` to it if it does.
    /// Otherwise, allocates a new block on the segment and creates a `SegSumEntry` using `n`.
    fn get_or_add_updated_block<
        C: Fn(&SegSumEntry) -> bool,
        N: FnOnce(BufUnlocked) -> SegSumEntry,
    >(
        &mut self,
        c: C,
        n: N,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        for i in (0..self.offset).rev() {
            if c(&self.segment_summary[i]) {
                return Some((
                    self.segment_summary[i].get_buf().unwrap().clone().lock(ctx),
                    self.get_disk_block_no(i + 1, ctx),
                ));
            }
        }
        self.add_new_block(n, ctx)
    }

    /// Provides a block on the segment to be used to store the updated inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// The returned `Buf` may be a buffer of a new zeroed block, or if the block was
    /// already requested before, the `Buf` may be a `Buf` to that if the segment was not committed afterwards.
    ///
    /// Whenever an inode gets updated, run this and write the new data to the returned `Buf`.
    ///
    /// # Note
    ///
    /// Use this only when updating an inode. For allocating a new inode, use `Segment::add_new_node_block` instead.
    pub fn get_or_add_updated_inode_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.get_or_add_updated_block(
            |entry| {
                if let SegSumEntry::Inode {
                    inum: inum2,
                    buf: _,
                } = entry
                {
                    inum == *inum2
                } else {
                    false
                }
            },
            |buf| SegSumEntry::Inode { inum, buf },
            ctx,
        )
    }

    /// Provides a block on the segment to be used to store the updated data block of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// The returned `Buf` may be a buffer of a new zeroed block, or if the block was
    /// already requested before, the `Buf` may be a `Buf` to that if the segment was not committed afterwards.
    ///
    /// Whenever a data block gets updated, run this and write the new data at the returned `Buf`.
    ///
    /// # Note
    ///
    /// Use this only when updating a data block. For allocating a new data block, use `Segment::add_new_data_block` instead.
    pub fn get_or_add_updated_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.get_or_add_updated_block(
            |entry| {
                if let SegSumEntry::DataBlock {
                    inum: inum2,
                    block_no: block_no2,
                    buf: _,
                } = entry
                {
                    inum == *inum2 && block_no == *block_no2
                } else {
                    false
                }
            },
            |buf| {
                SegSumEntry::DataBlock {
                    inum,
                    block_no,
                    buf,
                }
            },
            ctx,
        )
    }

    /// Provides a block on the segment to be used to store the updated indirect block of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// The returned `Buf` may be a buffer of a new zeroed block, or if the block was
    /// already requested before, the `Buf` may be a `Buf` to that if the segment was not committed afterwards.
    ///
    /// Whenever an inode's indirect data block's address changes, run this and write the new data at the returned `Buf`.
    ///
    /// # Note
    ///
    /// Use this only when updating a indirect block. For allocating a new indirect block, use `Segment::add_new_indirect_block` instead.
    pub fn get_or_add_updated_indirect_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.get_or_add_updated_block(
            |entry| {
                if let SegSumEntry::IndirectMap {
                    inum: inum2,
                    buf: _,
                } = entry
                {
                    inum == *inum2
                } else {
                    false
                }
            },
            |buf| SegSumEntry::IndirectMap { inum, buf },
            ctx,
        )
    }

    /// Provides a block on the segment to be used to store the updated an imap block.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// The returned `Buf` may be a buffer of a new zeroed block, or if the block was
    /// already requested before, the `Buf` may be a `Buf` to that if the segment was not committed afterwards.
    ///
    /// Whenever the imap gets updated, run this with the proper imap block number and write the new data at the returned `Buf`.
    pub fn get_or_add_updated_imap_block(
        &mut self,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.get_or_add_updated_block(
            |entry| {
                if let SegSumEntry::Imap {
                    block_no: block_no2,
                    buf: _,
                } = entry
                {
                    block_no == *block_no2
                } else {
                    false
                }
            },
            |buf| SegSumEntry::Imap { block_no, buf },
            ctx,
        )
    }

    /// Commits the segment to the disk. Updates the checkpoint region of the disk if needed.
    /// Run this when the segment is full or right before shutdowns.
    pub fn commit(&mut self, ctx: &KernelCtx<'_, '_>) {
        const_assert!(core::mem::size_of::<DSegSum>() <= BSIZE);

        // Get the segment summary.
        let mut bp = self.read_segment_block(0, ctx);
        let ssp = bp.deref_inner_mut().data.as_mut_ptr() as *mut DSegSum;
        unsafe { ptr::write(ssp, DSegSum::new(&self.segment_summary)) };

        // Collect the blocks' `Buf`s in one array.
        let mut barray: [Option<Buf>; SEGSIZE] = array![i => {
            if i > 0 && i <= self.segment_summary.len() {
                self.segment_summary[i - 1].get_buf().map(|b| b.clone().lock(ctx))
            } else {
                None
            }
        }; SEGSIZE];
        barray[0] = Some(bp);

        // Write all the buffers sequentially to the disk.
        hal().disk().write_sequential(&mut barray, ctx);

        for bopt in barray {
            if let Some(buf) = bopt {
                buf.free(ctx);
            }
        }

        self.segment_no = ctx.kernel().fs().get_next_seg_no(Some(self.segment_no));
        self.segment_summary = array![_ => SegSumEntry::Empty; SEGSIZE - 1];
        self.offset = 0;

        // TODO: Update the on-disk checkpoint if needed.
    }
}
