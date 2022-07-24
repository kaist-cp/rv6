//! Manages the in-memory segment.
//! Any kernel write operations to the disk must be done through this.
//!
//! # How to write something to the disk in the `Lfs`
//!
//! Any kernel write operations to the disk must be done using the `SegManager`'s methods in `Lfs`.
//! That is, when you want to write something new to the disk (ex: create a new inode)
//! or update something already on the disk (ex: update an inode/inode data block/imap),
//! you should
//! 1. lock the `SegManager`,
//! 2. request the `SegManager` for a `Buf`,
//! 3. write on the `Buf`, and
//! 4. commit the `SegManager` if it's full.
//!
//! # `add_new_*_block` vs `get_or_add_*_block`
//!
//! The `SegManager` has two types of methods that provides a `(Buf, u32)` pair to the outside.
//! * `add_new_*_block` methods : Always use these methods when allocating a **new**
//!   inode/inode data block/inode indirect block. These methods always allocate a new zeroed disk block.
//! * `get_or_add_*_block` methods : Always use these methods when **updating** a
//!   inode/inode data block/inode indirect block/imap block that already exists.
//!   These methods may allocate a new zeroed disk block or just return a `Buf` to a previously allocated
//!   block if it is still on the segment.
//!     * Using this method lets us reduce the cost to copy the content from an old buf to a new one every time
//!       we write something to the disk. However, using this method may cause a problem if this is the first
//!       time we allocate a disk block for the inode/inode data block/inode indirect map.
//!       If an inode with the same inum was previously finalized but has some of its blocks still on the segment,
//!       then this method may return one of these instead of a zeroed one. You must use these methods only for updates.
//!
//! # Updating the `Inode` or `Imap`
//!
//! The `SegManager`'s methods does not update the `Imap` by itself. Instead, everytime you use the previously
//! mentioned methods, it returns a `Buf` and a `u32` which holds the disk block number of it.
//! You should manually update the `Inode`'s `addr_direct`/`addr_indirect` field or `Imap`'s mapping
//! using the returned disk block number.
//!
//! # Lock order
//!
//! When acquiring the lock on the `SegManager`, `Imap`, or `Buf` at the same time, it must always done in the order of
//! `SegManager` -> `Imap` -> `Buf`. Otherwise, you may encounter a deadlock.

use arrayvec::ArrayVec;
use static_assertions::const_assert;

use crate::{
    bio::{Buf, BufUnlocked},
    hal::hal,
    param::{BSIZE, SEGSIZE, SEGTABLESIZE},
    proc::KernelCtx,
};

#[derive(PartialEq, Clone, Copy)]
/// Entries for the in-memory segment summary.
pub enum SegSumEntry {
    /// Inode.
    Inode { inum: u32 },
    /// Data block of an inode.
    DataBlock { inum: u32, block_no: u32 },
    /// The block that stores the mapping for all indirect data blocks of an inode.
    IndirectMap { inum: u32 },
    /// Imap.
    Imap { block_no: u32 },
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum BlockType {
    Empty,
    Inode,
    DataBlock,
    IndirectMap,
    Imap,
}

/// On-disk segment summary entry structure.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct DSegSumEntry {
    /// 0: empty, 1: inode, 2: data block, 3: indirect map, 4: imap block
    pub block_type: BlockType,
    pub inum: u32,     // 0 in case of empty or imap block
    pub block_no: u32, // 0 in case of inode or indirect map
}

impl Default for DSegSumEntry {
    fn default() -> Self {
        Self {
            block_type: BlockType::Empty,
            inum: 0,
            block_no: 0,
        }
    }
}

impl From<SegSumEntry> for DSegSumEntry {
    fn from(entry: SegSumEntry) -> Self {
        match entry {
            SegSumEntry::Inode { inum } => {
                Self {
                    block_type: BlockType::Inode,
                    inum,
                    block_no: 0,
                }
            }
            SegSumEntry::DataBlock { inum, block_no } => {
                Self {
                    block_type: BlockType::DataBlock,
                    inum,
                    block_no,
                }
            }
            SegSumEntry::IndirectMap { inum } => {
                Self {
                    block_type: BlockType::IndirectMap,
                    inum,
                    block_no: 0,
                }
            }
            SegSumEntry::Imap { block_no } => {
                Self {
                    block_type: BlockType::Imap,
                    inum: 0,
                    block_no,
                }
            }
        }
    }
}

/// On-disk segment summary structure.
#[derive(Clone)]
#[repr(C)]
pub struct DSegSum {
    pub magic: u32,
    pub size: u32,
    pub entries: [DSegSumEntry; SEGSIZE - 1],
}

impl Default for DSegSum {
    fn default() -> Self {
        Self {
            magic: SEGSUM_MAGIC,
            size: 0,
            entries: [DSegSumEntry::default(); SEGSIZE - 1],
        }
    }
}

pub const SEGSUM_MAGIC: u32 = 0x10305070;

/// The segment allocation table (bitmap).
pub type SegTable = [u8; SEGTABLESIZE];

/// Manages the in-memory segment.
/// Any kernel write operations to the disk must be done through the `SegManager`'s methods.
///
/// See the module documentation for details.
pub struct SegManager {
    dev_no: u32,

    /// The segment allocation table.
    segtable: SegTable,

    /// The total number of segments on the disk.
    nsegments: u32,

    /// The number of free segments.
    nfree: u32,

    /// The total number of blocks written to the segment since boot.
    /// Writing segment summary blocks or overwritting previously allocated blocks
    /// are not included.
    blocks_written: usize,

    /// The segment number of the current segment.
    segment_no: u32,

    /// The segment block number of the current segment summary block.
    start: usize,

    /// An `ArrayVec` where we store pairs of a `SegSumEntry` and `Buf`.
    /// A `SegSumEntry` describes a segment block
    /// and the `BufUnlocked` is the buffer of that segment block.
    segment: ArrayVec<(SegSumEntry, BufUnlocked), SEGSIZE>,

    /// An `ArrayVec` where we temporarily store the locked blocks that are about to be written to the disk.
    // TODO: We need this to prevent stack overflow. Remove after resolving stack overflow issue.
    locked_bufs: ArrayVec<Buf, SEGSIZE>,
}

impl SegManager {
    // TODO: Load from a non-empty segment instead?
    pub fn new(dev_no: u32, segtable: SegTable, nsegments: u32) -> Self {
        let mut this = Self {
            dev_no,
            segtable,
            nsegments,
            nfree: 0,
            blocks_written: 0,
            segment_no: 0,
            start: 0,
            segment: ArrayVec::new(),
            locked_bufs: ArrayVec::new(),
        };
        // Count the number of free segments.
        for i in 0..(nsegments as usize) {
            if this.segtable_is_free(i as u32) {
                this.nfree += 1;
            }
        }
        // Allocate a segment.
        this.alloc_segment(None);
        this
    }

    /// Returns the disk block number for the `seg_block_no`th block on this segment.
    /// `seg_block_no` starts from 0 to `SEGSIZE - 1`.
    fn get_disk_block_no(&self, seg_block_no: usize, ctx: &KernelCtx<'_, '_>) -> u32 {
        ctx.kernel()
            .fs()
            .superblock()
            .seg_to_disk_block_no(self.segment_no, seg_block_no as u32)
    }

    /// Reads the `seg_block_no`th block of this segment.
    /// `seg_block_no` starts from 0 to `SEGSIZE - 1`.
    fn read_segment_block(&self, seg_block_no: usize, ctx: &KernelCtx<'_, '_>) -> Buf {
        hal()
            .disk()
            .read(self.dev_no, self.get_disk_block_no(seg_block_no, ctx), ctx)
    }

    /// Returns true if the segment has no more remaining blocks.
    /// You should `commit` the segment if the segment is full.
    pub fn is_full(&self) -> bool {
        self.start + self.segment.len() == SEGSIZE - 1
    }

    /// Returns the number of remaining blocks on the segment.
    /// You should `commit` the segment if the segment has less blocks than you need.
    pub fn remaining(&self) -> usize {
        SEGSIZE - 1 - self.start - self.segment.len()
    }

    /// Returns the number of free segments on the disk.
    pub fn nfree(&self) -> u32 {
        self.nfree
    }

    /// Returns the total number of blocks written to the segment since boot.
    /// Writing segment summary blocks or overwritting previously allocated blocks
    /// are not included.
    pub fn blocks_written(&self) -> usize {
        self.blocks_written
    }

    /// Returns true if the `seg_no`th segment is free.
    /// Otherwise, returns false.
    pub fn segtable_is_free(&self, seg_no: u32) -> bool {
        self.segtable[seg_no as usize / 8] & (1 << (seg_no % 8)) == 0
    }

    /// Marks the `seg_no`th segment as allocated.
    /// Does not check whether the segment is marked as free or not.
    fn segtable_alloc(&mut self, seg_no: u32) {
        self.segtable[seg_no as usize / 8] |= 1 << (seg_no % 8);
    }

    /// Marks the `seg_no`th segment as free,
    /// and increments the number of free segments by 1.
    ///
    /// # Note
    ///
    /// You should call this function only when its sure that the
    /// `seg_no`th segment does not have any live blocks.
    /// Usually, you should call this function only inside the cleaner.
    ///
    /// # Panic
    ///
    /// Panics if the `seg_no`th segment is already marked as free.
    pub fn segtable_free(&mut self, seg_no: u32) {
        assert!(!self.segtable_is_free(seg_no));
        self.segtable[seg_no as usize / 8] &= !(1 << (seg_no % 8));
        self.nfree += 1;
    }

    /// Returns segment usage table in the on-disk format.
    /// This should be written at the checkpoint of the disk.
    pub fn dsegtable(&self) -> SegTable {
        self.segtable
    }

    /// Traverses the segment usage table to find an empty segment and marks it as used.
    /// Uses that segment from now on.
    /// If a `last_seg_no` was given, starts traversing from `last_seg_no + 1`.
    fn alloc_segment(&mut self, last_seg_no: Option<u32>) {
        let start = match last_seg_no {
            None => 0,
            Some(seg_no) => seg_no + 1,
        };
        for i in 0..self.nsegments {
            let seg_no = (start + i) % self.nsegments;
            if self.segtable_is_free(seg_no) {
                self.segtable_alloc(seg_no);
                self.segment_no = seg_no;
                self.nfree -= 1;
                return;
            }
        }
        panic!("no empty segment");
    }

    /// Allocates a new zeroed block on the segment to be used by the `entry`.
    /// Does not care if a block for the same inum/inode block number/imap block number already exists on the segment.
    /// By adding a new block, the previous one will no longer be returned by `SegManager`'s methods anyway.
    fn add_new_block(&mut self, entry: SegSumEntry, ctx: &KernelCtx<'_, '_>) -> Option<(Buf, u32)> {
        if self.is_full() {
            None
        } else {
            // Append segment with a new zeroed block.
            let block_no = self.start + 1 + self.segment.len();
            let mut buf = self.read_segment_block(block_no, ctx);
            buf.deref_inner_mut().data.fill(0);
            buf.deref_inner_mut().valid = true;
            self.segment.push((entry, buf.create_unlocked()));
            self.blocks_written += 1;
            Some((buf, self.get_disk_block_no(block_no, ctx)))
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
        self.add_new_block(SegSumEntry::Inode { inum }, ctx)
    }

    /// Provides a new zeroed block on the segment to be used to store a new data block of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// Always use this if this is the first time we allocate the `block_no`th data block for the inode.
    pub fn add_new_data_block(
        &mut self,
        inum: u32,
        block_no: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.add_new_block(SegSumEntry::DataBlock { inum, block_no }, ctx)
    }

    /// Provides a new zeroed block on the segment to be used to store the new indirect map of an inode.
    /// If succeeds, returns a `Buf` of the disk block and the disk block number of it.
    /// Always use this if this is the first time we allocate the indirect block for the inode.
    pub fn add_new_indirect_block(
        &mut self,
        inum: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        self.add_new_block(SegSumEntry::IndirectMap { inum }, ctx)
    }

    /// Checks if the block for the given `entry` already exists on the segment.
    /// If it does, returns a `Buf` to it and its disk block number.
    /// Otherwise, allocates a new block on the segment to be used by the `entry`.
    fn get_or_add_updated_block(
        &mut self,
        entry: SegSumEntry,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(Buf, u32)> {
        // Check if the block already exists.
        for i in (0..self.segment.len()).rev() {
            if self.segment[i].0 == entry {
                return Some((
                    self.segment[i].1.clone().lock(ctx),
                    self.get_disk_block_no(self.start + 1 + i, ctx),
                ));
            }
        }
        self.add_new_block(entry, ctx)
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
        self.get_or_add_updated_block(SegSumEntry::Inode { inum }, ctx)
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
        self.get_or_add_updated_block(SegSumEntry::DataBlock { inum, block_no }, ctx)
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
        self.get_or_add_updated_block(SegSumEntry::IndirectMap { inum }, ctx)
    }

    /// Provides a block on the segment to be used to store the updated imap block.
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
        self.get_or_add_updated_block(SegSumEntry::Imap { block_no }, ctx)
    }

    /// Commits the segment to the disk and allocates a new segment if necessary.
    /// If `alloc` is `true`, always allocates a new segment, unless the segment is empty.
    /// Run this when you need to empty the segment or before committing the checkpoint.
    pub fn commit(&mut self, alloc: bool, ctx: &KernelCtx<'_, '_>) {
        const_assert!(core::mem::size_of::<DSegSum>() <= BSIZE);

        let len = self.segment.len();
        if len > 0 {
            // Write the segment summary.
            let mut bp = self.read_segment_block(self.start, ctx);
            let ssp = unsafe { &mut *(bp.deref_inner_mut().data.as_mut_ptr() as *mut DSegSum) };
            ssp.magic = SEGSUM_MAGIC;
            ssp.size = self.segment.len() as u32;
            for i in 0..self.segment.len() {
                ssp.entries[i] = self.segment[i].0.into();
            }
            self.locked_bufs.push(bp);

            // Write all the `Buf`s sequentially to the disk, and then free them.
            for (_, buf) in self.segment.drain(..) {
                self.locked_bufs.push(buf.lock(ctx));
            }
            hal().disk().write_sequential(&mut self.locked_bufs, ctx);
            for buf in self.locked_bufs.drain(..) {
                buf.free(ctx);
            }

            // Update `self.start`.
            self.start += len + 1;
        }

        // Allocate a new segment if we need to.
        if alloc || self.start >= SEGSIZE - 1 {
            self.alloc_segment(Some(self.segment_no));
            self.start = 0;
        }
    }
}
