// use super::{Inode, Itable, Lfs};
use crate::param::SEGSIZE;

// TODO: replace BlockType with Block enum
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum BlockType {
    None,
    DataBlock,
    Inode,
    Itable,
}

// TODO: implement segment flush algorithms here
#[allow(dead_code)]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Segment {
    /// Buffer that holds updated blocks
    pub block_buffer: [BlockType; SEGSIZE],

    /// Current offset of the block_buffer
    pub offset: u32,
}

impl Segment {
    pub const fn default() -> Self {
        Segment {
            block_buffer: [BlockType::None; SEGSIZE],
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
}
