use super::{Itable, Lfs};
use crate::{
    fs::RcInode,
    param::{BSIZE, NBLOCK},
};

// TODO: BlockType should be replaced with SegSumEntry
// Inode should have only inum and block_no
// Imap, Data should be more simplified
pub enum BlockType {
    Invalid,
    Data { inner: [u32; BSIZE] },
    Inode { inner: RcInode<Lfs> },
    Imap { inner: Itable<Lfs> },
}

pub struct Block {
    pub typ: BlockType,
    segment_num: u32,
    offset: u32,
}

impl Block {
    pub const fn new(typ: BlockType, segment_num: u32, offset: u32) -> Self {
        Self {
            typ,
            segment_num,
            offset,
        }
    }
}

impl const Default for Block {
    fn default() -> Self {
        Self::new(BlockType::Invalid, 0, 0)
    }
}

// TODO: implement segment flush
#[repr(C)]
pub struct Segment {
    /// Current offset of the block_buffer
    pub offset: u32,

    /// Buffer that holds updated blocks
    pub block_buffer: [Block; NBLOCK],
}

impl Segment {
    pub const fn new(block_buffer: [Block; NBLOCK], offset: u32) -> Self {
        Segment {
            offset,
            block_buffer,
        }
    }
}

impl const Default for Segment {
    fn default() -> Self {
        Segment {
            offset: 0,
            block_buffer: [Block::default(); NBLOCK],
        }
    }
}
