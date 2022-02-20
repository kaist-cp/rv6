// use super::{Inode, Itable, Lfs};
use crate::param::SEGSIZE;

// TODO: replace BlockType with Block enum
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum BlockType {
    Invalid,
    DataBlock,
    Inode,
    Imap,
}

pub struct SegSumEntry {
    /// Inode number. If -1, indicates the inode map.
    inum: u16,

    /// Logical block number of the inode. If -1, indicates the inode itself.
    logical_block_no: i32
}

// TODO: implement segment flush algorithms here
#[allow(dead_code)]
#[derive(Copy, Clone)]
#[repr(C)]
/// Segment type. The unit of disk writes.
/// Note that we only hold the segment summary in memory.
/// When we flush the segment, we write the segment summary and its corresponding blocks to the disk altogether.
pub struct Segment {
    segment_no: u32,

    /// Segment summary. Indicates info for each entry of the block_buffer.
    segment_summary: [SegSumEntry; SEGSIZE],

    /// Current offset of the block_buffer
    offset: u32,
}

impl Segment {
    pub const fn default() -> Self {
        Segment {
            block_buffer: [BlockType::Invalid; SEGSIZE],
            offset: 0,
        }
    }

    #[allow(dead_code)]
    pub fn new(block_buffer: [BlockType; SEGSIZE], offset: u32) -> Self {
        Segment {
            block_buffer,
            offset,
        }
    }

    /// Appends the updated `Inode` info at the back of the `Segment` and updates the `InodeMap`.
    /// Flushes the Segment to the disk if needed.
    // TODO: Change to push_back_or_update_inode
    pub fn push_back_inode(&mut self, inode: &Inode, ctx: &KernelCtx<'_, '_>) {
        // Update Segment
        self.segment_summary[self.offset].inum = inode.inum;
        self.segment_summary[self.offset].logical_block_no = -1;
        // Update Inode Map
        ctx.imap().set(inode.inum, self.segment_no, self.offset);

        self.offset += 1;
        if self.offset == SEGSIZE {
            self.flush();
        }
    }

    /// Appends the `Inode`'s updated logical block at the back of the `Segment`.
    /// 
    /// # Note
    /// 
    /// You may need to call `Segment::push_back_inode` after calling this.
    pub fn push_back_data_block(&mut self, inode_guard: &mut InodeGuard<'_, LFS>, logical_block_no: u32) {
        // Update Segment
        self.segment_summary[self.offset].inum = inode_guard.inum;
        self.segment_summary[self.offset].logical_block_no = logical_block_no;
        // TODO: Update Inode's data block tree

        self.offset += 1;
        if self.offset == SEGSIZE {
            self.flush();
        }
    }

    pub fn push_back_inode_map(&mut self) {
        // Update Segment
        self.segment_summary[self.offset].inum = -1;
        self.segment_summary[self.offset].logical_block_no = -1;

        self.offset += 1;
        if self.offset == SEGSIZE {
            self.flush();
        }
    }

    fn flush(&mut self) {
        // TODO: write to disk
        // TODO: update segment_no
        self.offset = 0;
    }

}
