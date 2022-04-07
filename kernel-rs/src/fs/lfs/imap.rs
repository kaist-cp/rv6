use core::mem;

use static_assertions::const_assert;

use super::Segment;
use crate::{
    bio::{Buf, BufData},
    hal::hal,
    param::{BSIZE, IMAPSIZE},
    proc::KernelCtx,
};

// Number of entries in each on-disk imap block.
pub const NENTRY: usize = BSIZE / 4;

/// On-disk structure for each imap block.
/// Stores the disk block number for each inum.
#[repr(C)]
#[derive(Clone)]
struct DImapBlock {
    entry: [u32; NENTRY],
}

/// Stores the address of each imap block.
pub struct Imap {
    dev_no: u32,
    ninodes: usize,
    addr: [u32; IMAPSIZE],
}

impl Imap {
    pub fn new(dev_no: u32, ninodes: usize, addr: [u32; IMAPSIZE]) -> Self {
        Self {
            dev_no,
            ninodes,
            addr,
        }
    }

    /// For the inode with inode number `inum`,
    /// returns where the inode's mapping is stored in the imap in the form of (imap block number, offset within block).
    fn get_imap_block_no(&self, inum: u32) -> (usize, usize) {
        (inum as usize / NENTRY, inum as usize % NENTRY)
    }

    /// Returns the `block_no`th block of the imap.
    fn get_imap_block(&self, block_no: usize, ctx: &KernelCtx<'_, '_>) -> Buf {
        hal().disk().read(self.dev_no, self.addr[block_no], ctx)
    }

    /// Returns the imap in the on-disk format.
    /// This should be written at the checkpoint of the disk.
    pub fn dimap(&self) -> [u32; IMAPSIZE] {
        self.addr
    }

    /// Returns an unused inum.
    pub fn get_empty_inum(&self, ctx: &KernelCtx<'_, '_>) -> Option<u32> {
        for i in 0..IMAPSIZE {
            let buf = self.get_imap_block(i, ctx);
            let imap_block = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DImapBlock) };
            for j in 0..NENTRY {
                let inum = i * NENTRY + j;
                // inum: (0, ninodes)
                if inum != 0 && inum < self.ninodes && imap_block.entry[j] == 0 {
                    buf.free(ctx);
                    return Some(inum as u32);
                }
            }
            buf.free(ctx);
        }
        None
    }

    /// For the inode with inode number `inum`, returns the disk_block_no of it.
    pub fn get(&self, inum: u32, ctx: &KernelCtx<'_, '_>) -> u32 {
        assert!(
            inum < ctx.kernel().fs().superblock().ninodes(),
            "invalid inum"
        );
        let (block_no, offset) = self.get_imap_block_no(inum);
        let buf = self.get_imap_block(block_no, ctx);

        const_assert!(mem::size_of::<DImapBlock>() <= mem::size_of::<BufData>());
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<DImapBlock>() == 0);
        let imap_block = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DImapBlock) };
        let res = imap_block.entry[offset];
        buf.free(ctx);
        res
    }

    /// For the inode with inode number `inum`, updates its mapping in the imap to disk_block_no.
    /// Then, we append the new imap block to the segment.
    /// Returns true if successful. Otherwise, returns false.
    pub fn set(
        &mut self,
        inum: u32,
        disk_block_no: u32,
        segment: &mut Segment,
        ctx: &KernelCtx<'_, '_>,
    ) -> bool {
        assert!(
            inum < ctx.kernel().fs().superblock().ninodes(),
            "invalid inum"
        );
        let (block_no, offset) = self.get_imap_block_no(inum);

        if let Some((mut buf, addr)) = segment.get_or_add_updated_imap_block(block_no as u32, ctx) {
            if addr != self.addr[block_no] {
                // Copy the imap block content from old imap block.
                let old_buf = self.get_imap_block(block_no, ctx);
                buf.deref_inner_mut()
                    .data
                    .copy_from(&old_buf.deref_inner().data);
                // Update imap mapping.
                self.addr[block_no] = addr;
                old_buf.free(ctx);
            }
            // Update entry.
            const_assert!(mem::size_of::<DImapBlock>() <= mem::size_of::<BufData>());
            const_assert!(mem::align_of::<BufData>() % mem::align_of::<DImapBlock>() == 0);
            let imap_block =
                unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut DImapBlock) };
            imap_block.entry[offset] = disk_block_no;
            buf.free(ctx);
            true
        } else {
            false
        }
    }
}
