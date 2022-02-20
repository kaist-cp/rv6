// TODO: Every time a transaction (`Tx`) ends, should update the checkpoint to indicate what is the latest segment,
// and up to what block it is valid.
// TODO: What if a single transaction takes more than a single segment?

use crate::param::NINODE;

/// Checkpoint. 
/// Stored at two fixed positions on disk.
/// Stores the location of the inode map, segment usage table,
/// and indicates the latest segment and its last valid block.
struct Checkpoint {
    last_segment: u32,
    last_segment_block: u32,
}

struct InodeMapEntry {
    seg_no: u32,
    seg_block_no: u32,
}

/// Simple translation of inode_number -> (segment_number, segment_block_number).
/// 
/// # Note
/// 
/// The in-memory `Inode`s are stored on the `Itable`, not here.
// TODO: Synchronization?
pub struct InodeMap {
    entry: [InodeMapEntry; NINODE],
}

impl InodeMap {
    /// Load the InodeMap from the Checkpoint region.
    pub fn new() -> InodeMap {
        !todo()
    }

    /// For the inode with inode number `inum`,
    /// returns the segment number and segment block number.
    pub fn get(&self, inum: u32) -> (u32, u32) {
        assert!(inum < NINODE, "invalid inum");
        (entry[inum].seg_no, entry[inum].seg_block_no)
    }

    /// For the inode with inode number `inum`,
    /// updates the mapping for its segment number and segment block number.
    /// 
    /// # Note
    /// 
    /// This should be used only by the `Segment`.
    pub fn set(&mut self, inum: u32, seg_no: u32, seg_block_no: u32) {
        assert!(inum < NINODE, "invalid inum");
        entry[inum].seg_no = seg_no;
        entry[inum].seg_block_nono = seg_block_no;
    }
}