use crate::param::SEGSIZE;
use core::convert::TryInto;

/// Entries for the in-memory segment summary.
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum SegSumEntry {
    Invalid,
    Inode { inum: u32 },
    DataBlock { inum: u32, block_no: u32 },
    Imap,
}

/// On-disk segment summary entry structure.
// TODO: Do we need this? We could just make this `SegSumEntry` instead.
#[repr(C)]
#[derive(Copy, Clone)]
struct DSegSumEntry {
    block_type: u32,
    inum: u32,
    block_no: u32,
}

/// On-disk segment summary structure.
#[allow(dead_code)]
type DSegSum = [DSegSumEntry; SEGSIZE];

// TODO: implement segment flush algorithms here
#[allow(dead_code)]
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
    /// The segment number of this segment.
    segment_no: u32,

    /// Segment summary. Indicates info for each block that should be in the segment.
    segment_summary: [SegSumEntry; SEGSIZE],

    /// Current offset of the segment. Must flush when `offset == SEGSIZE`.
    offset: usize,

    /// 0 if the imap is not on the segment.
    /// Otherwise, indicates the block number of the segment where the imap is located.
    imap_block_no: u32,
}

impl Segment {
    pub const fn default() -> Self {
        Segment {
            segment_no: 0,
            segment_summary: [SegSumEntry::Invalid; SEGSIZE],
            offset: 0,
            imap_block_no: 0,
        }
    }

    #[allow(dead_code)]
    pub fn new(segment_no: u32, segment_summary: [SegSumEntry; SEGSIZE], offset: usize, imap_block_no: u32) -> Self {
        Segment {
            segment_no,
            segment_summary,
            offset,
            imap_block_no,
        }
    }

    /// Pushes the `entry` at the back of the segment summary.
    /// This is logically appending the block at the back of the segment.
    #[allow(dead_code)]
    fn push_back_block(&mut self, entry: SegSumEntry) -> Option<(u32, u32)> {
        if self.offset == SEGSIZE {
            // Segment is full.
            None
        } else {
            // Append segment.
            self.segment_summary[self.offset] = entry;
            self.offset += 1;
            Some((self.segment_no, (self.offset - 1).try_into().unwrap()))
        }
    }

    /// Appends the inode at the back of the segment if it is not already on the segment.
    /// Returns the location as (segment number, segment block number) if succeeded. Otherwise, returns `None`.
    // TODO: Change u32 -> &Inode to ensure inode is in-memory?
    #[allow(dead_code)]
    pub fn push_back_inode(&mut self, inum: u32) -> Option<(u32, u32)> {
        // Check if the block already exists.
        // TODO: Maybe more efficient if we make the imap bookmark this.
        for i in 0..self.offset {
            if let SegSumEntry::Inode { inum: inode_number } = self.segment_summary[i] {
                if inode_number == inum {
                    return Some((self.segment_no, i.try_into().unwrap()));
                }
            }
        }
        self.push_back_block(SegSumEntry::Inode { inum })
    }

    /// Appends the inode's updated data block at the back of the segment if it is not already on the segment.
    /// Returns the location as (segment number, segment block number) if succeeded. Otherwise, returns `None`.
    /// 
    /// # Note
    /// 
    /// You may need to call `Segment::push_back_inode` or `Segment::push_back_data_block` for another block after calling this.
    // TODO: Add `Buf` as argument?
    #[allow(dead_code)]
    pub fn push_back_data_block(&mut self, inum: u32, block_no: u32) -> Option<(u32, u32)> {
        // Check if the block already exists.
        for i in 0..self.offset {
            if let SegSumEntry::DataBlock { inum: inode_number, block_no: block_number } = self.segment_summary[i] {
                if inode_number == inum && block_number == block_no {
                    return Some((self.segment_no, i.try_into().unwrap()));
                }
            }
        }
        self.push_back_block(SegSumEntry::DataBlock { inum, block_no })
    }

    /// Appends the inode's updated data block at the back of the segment if it is not already on the segment.
    /// Returns the location as (segment number, segment block number) if succeeded. Otherwise, returns `None`.
    #[allow(dead_code)]
    pub fn push_back_inode_map(&mut self) -> Option<(u32, u32)> {
        if self.imap_block_no == 0 {
            let result = self.push_back_block(SegSumEntry::Imap);
            if let Some((_, block_no)) = result {
                self.imap_block_no = block_no;
            }
            result
        } else {
            Some((self.segment_no, self.imap_block_no))
        }
    }

    // TODO: Use MaybeUninit
    #[allow(dead_code)]
    fn create_dsegsum(&self) -> DSegSum {
        let mut d_seg_sum = [DSegSumEntry { block_type: 0, inum: 0, block_no: 0 }; SEGSIZE];
        for i in 0..SEGSIZE {
            d_seg_sum[i] = match self.segment_summary[i] {
                SegSumEntry::Invalid => DSegSumEntry{ block_type: 0, inum: 0, block_no: 0 },
                SegSumEntry::Inode{ inum } => DSegSumEntry{ block_type: 1, inum, block_no: 0 },
                SegSumEntry::DataBlock{ inum , block_no } => DSegSumEntry{ block_type: 2, inum, block_no },
                SegSumEntry::Imap => DSegSumEntry{ block_type: 3, inum: 0, block_no: 0 },
            };
        }
        d_seg_sum
    }

    #[allow(dead_code)]
    fn flush(&mut self) {
        let _dsegsum = self.create_dsegsum();
        // TODO: write dsegsum and segment blocks to the disk

        // TODO: fix this
        self.segment_no += 1;

        self.offset = 0;
    }

}
