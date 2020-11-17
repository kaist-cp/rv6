//! File system implementation.  Five layers:
//!   + Blocks: allocator for raw disk blocks.
//!   + Log: crash recovery for multi-step updates.
//!   + Files: inode allocator, reading, writing, metadata.
//!   + Directories: inode with special contents (list of other inodes!)
//!   + Names: paths like /usr/rtm/xv6/fs.c for convenient naming.
//!
//! This file contains the low-level file system manipulation
//! routines.  The (higher-level) system call implementations
//! are in sysfile.c.
//!
//! On-disk file system format used for both kernel and user programs are also included here.

use core::{mem, ptr};

use crate::{
    bio::BufUnlocked, param::BSIZE, sleepablelock::Sleepablelock, stat::T_DIR, virtio_disk::Disk,
};

mod inode;
mod log;
mod path;
mod superblock;

pub use inode::{Dinode, Dirent, Inode, InodeGuard, InodeInner, RcInode, DIRENT_SIZE, DIRSIZ};
pub use log::Log;
pub use path::{FileName, Path};
pub use superblock::{Superblock, BPB, IPB};

/// root i-number
const ROOTINO: u32 = 1;

const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

pub struct FileSystem {
    /// there should be one superblock per disk device, but we run with
    /// only one device
    superblock: Superblock,

    /// TODO(rv6): document it
    log: Sleepablelock<Log>,
}

pub struct FsTransaction<'s> {
    fs: &'s FileSystem,
}

impl Drop for FsTransaction<'_> {
    fn drop(&mut self) {
        // Called at the end of each FS system call.
        // Commits if this was the last outstanding operation.
        unsafe {
            Log::end_op(&self.fs.log);
        }
    }
}

impl FileSystem {
    pub fn new(dev: u32) -> Self {
        let superblock = unsafe { Superblock::new(&Disk::read(dev, 1)) };
        let log = Sleepablelock::new("LOG", Log::new(dev, &superblock));
        Self { superblock, log }
    }

    /// Called for each FS system call.
    pub fn begin_transaction(&self) -> FsTransaction<'_> {
        // TODO(rv6): safety?
        unsafe {
            Log::begin_op(&self.log);
        }
        FsTransaction { fs: self }
    }

    /// Zero a block.
    unsafe fn bzero(&self, dev: u32, bno: u32) {
        let mut buf = BufUnlocked::new(dev, bno).lock();
        ptr::write_bytes(buf.deref_mut_inner().data.as_mut_ptr(), 0, BSIZE);
        buf.deref_mut_inner().valid = true;
        self.log.lock().write(buf);
    }

    /// Blocks.
    /// Allocate a zeroed disk block.
    unsafe fn balloc(&self, dev: u32) -> u32 {
        let mut bi: u32 = 0;
        for b in num_iter::range_step(0, self.superblock.size, BPB) {
            let mut bp = Disk::read(dev, self.superblock.bblock(b));
            while bi < BPB && (b + bi) < self.superblock.size {
                let m = 1 << (bi % 8);
                if bp.deref_mut_inner().data[(bi / 8) as usize] & m == 0 {
                    // Is block free?
                    bp.deref_mut_inner().data[(bi / 8) as usize] |= m; // Mark block in use.
                    self.log.lock().write(bp);
                    self.bzero(dev, b + bi);
                    return b + bi;
                }
                bi += 1;
            }
        }
        panic!("balloc: out of blocks");
    }

    /// Free a disk block.
    unsafe fn bfree(&self, dev: i32, b: u32) {
        let mut bp = Disk::read(dev as u32, self.superblock.bblock(b));
        let bi = b.wrapping_rem(BPB) as i32;
        let m = 1u8 << (bi % 8);
        assert_ne!(
            bp.deref_mut_inner().data[(bi / 8) as usize] & m,
            0,
            "freeing free block"
        );
        bp.deref_mut_inner().data[(bi / 8) as usize] &= !m;
        self.log.lock().write(bp);
    }
}
