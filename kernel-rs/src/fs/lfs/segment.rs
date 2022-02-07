use super::Inode;
use crate::param::{BSIZE, SEGSIZE};

const NBOCKS: usize = SEGSIZE / BSIZE;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct Segment {
    /// Buffer that holds updated blocks
    pub block_buffer: [u32; NBOCKS],

    /// Current offset of the block_buffer
    pub offset: u32,
}