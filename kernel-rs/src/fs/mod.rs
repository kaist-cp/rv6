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

use core::{cmp, mem};

use spin::Once;

use crate::{
    bio::Buf, kernel::kernel, param::BSIZE, sleepablelock::Sleepablelock, virtio_disk::Disk,
};

mod inode;
mod log;
mod path;
mod superblock;

pub use inode::{
    Dinode, Dirent, Inode, InodeGuard, InodeInner, InodeType, Itable, RcInode, DIRENT_SIZE, DIRSIZ,
};
pub use log::Log;
pub use path::{FileName, Path};
pub use superblock::{Superblock, BPB, IPB};

/// root i-number
const ROOTINO: u32 = 1;

const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

pub struct FileSystem {
    /// TODO(https://github.com/kaist-cp/rv6/issues/358)
    /// Initializing superblock should be run only once because forkret() calls fsinit()
    /// There should be one superblock per disk device, but we run with
    /// only one device.
    superblock: Once<Superblock>,

    /// TODO(https://github.com/kaist-cp/rv6/issues/358)
    /// document it / initializing log should be run
    /// only once because forkret() calls fsinit()
    log: Once<Sleepablelock<Log>>,

    /// It may sleep until some Descriptors are freed.
    pub disk: Sleepablelock<Disk>,
}

pub struct FsTransaction<'s> {
    fs: &'s FileSystem,
}

impl FileSystem {
    pub const fn zero() -> Self {
        Self {
            superblock: Once::new(),
            log: Once::new(),
            disk: Sleepablelock::new("virtio_disk", Disk::zero()),
        }
    }

    pub fn init(&self, dev: u32) {
        let _ = self
            .superblock
            .call_once(|| Superblock::new(&self.disk.read(dev, 1)));
        let _ = self.log.call_once(|| {
            Sleepablelock::new(
                "LOG",
                Log::new(
                    dev,
                    self.superblock().logstart as i32,
                    self.superblock().nlog as i32,
                ),
            )
        });
    }

    /// TODO(https://github.com/kaist-cp/rv6/issues/358)
    /// Calling superblock() after initialize is safe
    fn superblock(&self) -> &Superblock {
        if let Some(sb) = self.superblock.get() {
            sb
        } else {
            unreachable!()
        }
    }

    /// TODO(https://github.com/kaist-cp/rv6/issues/358)
    /// Calling log() after initialize is safe
    fn log(&self) -> &Sleepablelock<Log> {
        if let Some(log) = self.log.get() {
            log
        } else {
            unreachable!()
        }
    }

    /// Called for each FS system call.
    pub fn begin_transaction(&self) -> FsTransaction<'_> {
        Log::begin_op(self.log());
        FsTransaction { fs: self }
    }
}

impl Drop for FsTransaction<'_> {
    fn drop(&mut self) {
        // Called at the end of each FS system call.
        // Commits if this was the last outstanding operation.
        Log::end_op(self.fs.log());
    }
}

impl FsTransaction<'_> {
    /// Caller has modified b->data and is done with the buffer.
    /// Record the block number and pin in the cache by increasing refcnt.
    /// commit()/write_log() will do the disk write.
    ///
    /// write() replaces write(); a typical use is:
    ///   bp = kernel().file_system.disk.read(...)
    ///   modify bp->data[]
    ///   write(bp)
    fn write(&self, b: Buf<'static>) {
        self.fs.log().lock().write(b);
    }

    /// Zero a block.
    fn bzero(&self, dev: u32, bno: u32) {
        let mut buf = kernel().get_bcache().get_buf(dev, bno).lock();
        buf.deref_inner_mut().data.fill(0);
        buf.deref_inner_mut().valid = true;
        self.write(buf);
    }

    /// Blocks.
    /// Allocate a zeroed disk block.
    fn balloc(&self, dev: u32) -> u32 {
        for b in num_iter::range_step(0, self.fs.superblock().size, BPB as u32) {
            let mut bp = self.fs.disk.read(dev, self.fs.superblock().bblock(b));
            for bi in 0..cmp::min(BPB as u32, self.fs.superblock().size - b) {
                let m = 1 << (bi % 8);
                if bp.deref_inner_mut().data[(bi / 8) as usize] & m == 0 {
                    // Is block free?
                    bp.deref_inner_mut().data[(bi / 8) as usize] |= m; // Mark block in use.
                    self.write(bp);
                    self.bzero(dev, b + bi);
                    return b + bi;
                }
            }
        }

        panic!("balloc: out of blocks");
    }

    /// Free a disk block.
    fn bfree(&self, dev: u32, b: u32) {
        let mut bp = self.fs.disk.read(dev, self.fs.superblock().bblock(b));
        let bi = b as usize % BPB;
        let m = 1u8 << (bi % 8);
        assert_ne!(
            bp.deref_inner_mut().data[bi / 8] & m,
            0,
            "freeing free block"
        );
        bp.deref_inner_mut().data[bi / 8] &= !m;
        self.write(bp);
    }
}
