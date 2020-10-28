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

/// On-disk file system format used for both kernel and user programs are also included here.
use crate::{
    bio::Buf,
    kernel::kernel,
    log::Log,
    sleepablelock::Sleepablelock,
    stat::T_DIR,
    vm::{KVAddr, VAddr},
};
use core::{mem, ops::DerefMut, ptr};

mod path;
pub use path::{FileName, Path};
mod inode;
pub use inode::{Dinode, Inode, InodeGuard, InodeInner, RcInode};

/// Disk layout:
/// [ boot block | super block | log | inode blocks |
///                                          free bit map | data blocks]
///
/// mkfs computes the super block and builds an initial file system. The
/// super block describes the disk layout:
#[derive(Copy, Clone)]
pub struct Superblock {
    /// Must be FSMAGIC
    magic: u32,

    /// Size of file system image (blocks)
    size: u32,

    /// Number of data blocks
    nblocks: u32,

    /// Number of inodes
    ninodes: u32,

    /// Number of log blocks
    pub nlog: u32,

    /// Block number of first log block
    pub logstart: u32,

    /// Block number of first inode block
    inodestart: u32,

    /// Block number of first free map block
    bmapstart: u32,
}

/// dirent size
pub const DIRENT_SIZE: usize = mem::size_of::<Dirent>();

#[derive(Default)]
pub struct Dirent {
    pub inum: u16,
    name: [u8; DIRSIZ],
}

impl Dirent {
    /// Fill in name. If name is shorter than DIRSIZ, NUL character is appended as
    /// terminator.
    ///
    /// `name` must contains no NUL characters, but this is not a safety invariant.
    fn set_name(&mut self, name: &FileName) {
        let name = name.as_bytes();
        if name.len() == DIRSIZ {
            self.name.copy_from_slice(&name);
        } else {
            self.name[..name.len()].copy_from_slice(&name);
            self.name[name.len()] = 0;
        }
    }

    /// Returns slice which exactly contains `name`.
    ///
    /// It contains no NUL characters.
    fn get_name(&self) -> &FileName {
        let len = self.name.iter().position(|ch| *ch == 0).unwrap_or(DIRSIZ);
        unsafe { FileName::from_bytes(&self.name[..len]) }
    }

    // TODO: Use iterator
    fn read_entry(&mut self, ip: &mut InodeGuard<'_>, off: u32, panic_msg: &'static str) {
        unsafe {
            let bytes_read = ip.read(
                KVAddr::wrap(self as *mut Dirent as usize),
                off,
                DIRENT_SIZE as u32,
            );
            assert_eq!(bytes_read, Ok(DIRENT_SIZE), "{}", panic_msg)
        }
    }
}

/// root i-number
const ROOTINO: u32 = 1;

/// block size
pub const BSIZE: usize = 1024;
const FSMAGIC: u32 = 0x10203040;
const NDIRECT: usize = 12;

const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

/// Inodes per block.
const IPB: usize = BSIZE.wrapping_div(mem::size_of::<Dinode>());

impl Superblock {
    /// Block containing inode i
    const fn iblock(self, i: u32) -> u32 {
        i.wrapping_div(IPB as u32).wrapping_add(self.inodestart)
    }

    /// Block of free map containing bit for block b
    const fn bblock(self, b: u32) -> u32 {
        b.wrapping_div(BPB).wrapping_add(self.bmapstart)
    }

    /// Read the super block.
    unsafe fn new(dev: i32) -> Self {
        let mut result = mem::MaybeUninit::uninit();
        let mut bp = Buf::new(dev as u32, 1);
        ptr::copy(
            bp.deref_mut_inner().data.as_mut_ptr(),
            result.as_mut_ptr() as *mut Superblock as *mut u8,
            mem::size_of::<Superblock>(),
        );
        result.assume_init()
    }
}

/// Bitmap bits per block
const BPB: u32 = BSIZE.wrapping_mul(8) as u32;

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

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
        unsafe {
            Log::end_op(&self.fs.log);
        }
    }
}

impl FileSystem {
    fn new(dev: i32) -> Self {
        unsafe {
            let superblock = Superblock::new(dev);
            assert_eq!(superblock.magic, FSMAGIC, "invalid file system");
            let log = Sleepablelock::new("LOG", Log::new(dev, &superblock));
            Self { superblock, log }
        }
    }

    /// Called for each FS system call.
    pub fn begin_transaction(&self) -> FsTransaction<'_> {
        // TODO(rv6): safety?
        unsafe {
            Log::begin_op(&self.log);
        }
        FsTransaction { fs: self }
    }

    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub unsafe fn end_op(&self) {
        Log::end_op(&self.log);
    }

    pub unsafe fn log_write(&self, b: Buf) {
        self.log.lock().log_write(b);
    }
}

pub fn fsinit(dev: i32) {
    kernel().file_system.call_once(|| FileSystem::new(dev));
}

pub fn fs() -> &'static FileSystem {
    if let Some(fs) = kernel().file_system.r#try() {
        fs
    } else {
        unreachable!()
    }
}

/// Zero a block.
unsafe fn bzero(dev: i32, bno: i32) {
    let mut bp = Buf::new(dev as u32, bno as u32);
    ptr::write_bytes(bp.deref_mut_inner().data.as_mut_ptr(), 0, BSIZE);
    fs().log_write(bp);
}

/// Blocks.
/// Allocate a zeroed disk block.
unsafe fn balloc(dev: u32) -> u32 {
    let mut b: u32 = 0;
    let mut bi: u32 = 0;
    while b < fs().superblock.size {
        let mut bp = Buf::new(dev, fs().superblock.bblock(b));
        while bi < BPB && (b + bi) < fs().superblock.size {
            let m = (1) << (bi % 8);
            if bp.deref_mut_inner().data[(bi / 8) as usize] as i32 & m == 0 {
                // Is block free?
                bp.deref_mut_inner().data[(bi / 8) as usize] =
                    (bp.deref_mut_inner().data[(bi / 8) as usize] as i32 | m) as u8; // Mark block in use.
                fs().log_write(bp);
                bzero(dev as i32, (b + bi) as i32);
                return b + bi;
            }
            bi += 1
        }
        b += BPB
    }
    panic!("balloc: out of blocks");
}

/// Free a disk block.
unsafe fn bfree(dev: i32, b: u32) {
    let mut bp = Buf::new(dev as u32, fs().superblock.bblock(b));
    let bi: i32 = b.wrapping_rem(BPB) as i32;
    let m: i32 = (1) << (bi % 8);
    assert_ne!(
        bp.deref_mut_inner().data[(bi / 8) as usize] as i32 & m,
        0,
        "freeing free block"
    );
    bp.deref_mut_inner().data[(bi / 8) as usize] =
        (bp.deref_mut_inner().data[(bi / 8) as usize] as i32 & !m) as u8;
    fs().log_write(bp);
}
