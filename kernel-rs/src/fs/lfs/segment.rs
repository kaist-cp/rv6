// use super::{Inode, Itable, Lfs};
use crate::param::SEGSIZE;

// TODO: replace BlockType with Block enum
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum BlockType {
    Invalid,
    DataBlock,
    Inode,
    Itable,
}

// TODO: implement segment flush algorithms here
#[allow(dead_code)]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Segment {
    /// Current offset of the block_buffer
    pub offset: u32,
    
    /// Buffer that holds updated blocks
    pub block_buffer: [BlockType; SEGSIZE],
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
}
