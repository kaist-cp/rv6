use core::{mem, ptr};

use static_assertions::const_assert;

use crate::{
    bio::{Buf, BufData},
    param::BSIZE,
};

const FSMAGIC: u32 = 0xfeedbacc;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct Superblock {
    /// Must be FSMAGIC
    magic: u32,

    /// Size of a segment (blocks)
    pub size_segment: u32,

    /// Size of a checkpoint (segments)
    pub size_checkpoint: u32,

    /// Number of log blocks
    pub nlog: u32,
}

impl Superblock {
    /// Read the super block.
    pub fn new(buf: &Buf) -> Self {
        const_assert!(mem::size_of::<Superblock>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<Superblock>() == 0);
        // SAFETY:
        // * buf.data is larger than Superblock
        // * buf.data is aligned properly.
        // * Superblock contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let result = unsafe { ptr::read(buf.deref_inner().data.as_ptr() as *const Superblock) };
        assert_eq!(result.magic, FSMAGIC, "invalid file system");
        result
    }
}
