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
use crate::{bio::brelease, buf::Buf, log::Log, param::NINODE, spinlock::Spinlock, stat::T_DIR};
use core::{mem, ops::DerefMut, ptr};

mod path;
pub use path::{FileName, Path};
mod inode;
pub use inode::{Inode, InodeGuard, InodeInner};
use spin::Once;

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
            let bytes_read = ip.read(0, self as *mut Dirent as usize, off, DIRENT_SIZE as u32);
            assert_eq!(bytes_read, Ok(DIRENT_SIZE), "{}", panic_msg)
        }
    }
}

/// On-disk inode structure
/// Both the kernel and user programs use this header file.
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct Dinode {
    /// File type
    typ: i16,

    /// Major device number (T_DEVICE only)
    major: u16,

    /// Minor device number (T_DEVICE only)
    minor: u16,

    /// Number of links to inode in file system
    nlink: i16,

    /// Size of file (bytes)
    size: u32,

    /// Data block addresses
    addrs: [u32; 13],
}

/// Inodes.
///
/// An inode describes a single unnamed file.
/// The inode disk structure holds metadata: the file's type,
/// its size, the number of links referring to it, and the
/// list of blocks holding the file's content.
///
/// The inodes are laid out sequentially on disk at
/// FS.superblock.startinode. Each inode has a number, indicating its
/// position on the disk.
///
/// The kernel keeps a cache of in-use inodes in memory
/// to provide a place for synchronizing access
/// to inodes used by multiple processes. The cached
/// inodes include book-keeping information that is
/// not stored on disk: ip->ref and ip->valid.
///
/// An inode and its in-memory representation go through a
/// sequence of states before they can be used by the
/// rest of the file system code.
///
/// * Allocation: an inode is allocated if its type (on disk)
///   is non-zero. Inode::alloc() allocates, and Inode::put() frees if
///   the reference and link counts have fallen to zero.
///
/// * Referencing in cache: an entry in the inode cache
///   is free if ip->ref is zero. Otherwise ip->ref tracks
///   the number of in-memory pointers to the entry (open
///   files and current directories). iget() finds or
///   creates a cache entry and increments its ref; Inode::put()
///   decrements ref.
///
/// * Valid: the information (type, size, &c) in an inode
///   cache entry is only correct when ip->valid is 1.
///   Inode::lock() reads the inode from
///   the disk and sets ip->valid, while Inode::put() clears
///   ip->valid if ip->ref has fallen to zero.
///
/// * Locked: file system code may only examine and modify
///   the information in an inode and its content if it
///   has first locked the inode.
///
/// Thus a typical sequence is:
///   ip = iget(dev, inum)
///   (*ip).lock()
///   ... examine and modify ip->xxx ...
///   (*ip).unlock()
///   (*ip).put()
///
/// Inode::lock() is separate from iget() so that system calls can
/// get a long-term reference to an inode (as for an open file)
/// and only lock it for short periods (e.g., in read()).
/// The separation also helps avoid deadlock and races during
/// pathname lookup. iget() increments ip->ref so that the inode
/// stays cached and pointers to it remain valid.
///
/// Many internal file system functions expect the caller to
/// have locked the inodes involved; this lets callers create
/// multi-step atomic operations.
///
/// The ICACHE.lock spin-lock protects the allocation of icache
/// entries. Since ip->ref indicates whether an entry is free,
/// and ip->dev and ip->inum indicate which i-node an entry
/// holds, one must hold ICACHE.lock while using any of those fields.
///
/// An ip->lock sleep-lock protects all ip-> fields other than ref,
/// dev, and inum.  One must hold ip->lock in order to
/// read or write that inode's ip->valid, ip->size, ip->type, &c.

static mut ICACHE: Spinlock<[Inode; NINODE]> = Spinlock::new("ICACHE", [Inode::zeroed(); NINODE]);

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
        let bp: *mut Buf = Buf::read(dev as u32, 1);
        ptr::copy(
            (*bp).inner.data.as_mut_ptr(),
            result.as_mut_ptr() as *mut Superblock as *mut u8,
            mem::size_of::<Superblock>(),
        );
        brelease(&mut *bp);
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
    log: Log,
}

impl FileSystem {
    fn new(dev: i32) -> Self {
        unsafe {
            let superblock = Superblock::new(dev);
            assert_eq!(superblock.magic, FSMAGIC, "invalid file system");
            let log = Log::new(dev, &superblock);
            Self { superblock, log }
        }
    }

    pub unsafe fn begin_op(&self) {
        #[allow(clippy::cast_ref_to_mut)]
        (*(self as *const _ as *mut Self)).log.begin_op();
    }

    pub unsafe fn end_op(&self) {
        #[allow(clippy::cast_ref_to_mut)]
        (*(self as *const _ as *mut Self)).log.end_op();
    }

    pub unsafe fn log_write(&self, b: *mut Buf) {
        #[allow(clippy::cast_ref_to_mut)]
        (*(self as *const _ as *mut Self)).log.log_write(b);
    }
}

static FS: Once<FileSystem> = Once::new();

pub fn fsinit(dev: i32) {
    FS.call_once(|| FileSystem::new(dev));
}

pub fn fs() -> &'static FileSystem {
    if let Some(fs) = FS.r#try() {
        fs
    } else {
        unreachable!()
    }
}

/// Zero a block.
unsafe fn bzero(dev: i32, bno: i32) {
    let bp: *mut Buf = Buf::read(dev as u32, bno as u32);
    ptr::write_bytes((*bp).inner.data.as_mut_ptr(), 0, BSIZE);
    fs().log_write(bp);
    brelease(&mut *bp);
}

/// Blocks.
/// Allocate a zeroed disk block.
unsafe fn balloc(dev: u32) -> u32 {
    let mut b: u32 = 0;
    let mut bi: u32 = 0;
    while b < fs().superblock.size {
        let mut bp: *mut Buf = Buf::read(dev, fs().superblock.bblock(b));
        while bi < BPB && (b + bi) < fs().superblock.size {
            let m = (1) << (bi % 8);
            if (*bp).inner.data[(bi / 8) as usize] as i32 & m == 0 {
                // Is block free?
                (*bp).inner.data[(bi / 8) as usize] =
                    ((*bp).inner.data[(bi / 8) as usize] as i32 | m) as u8; // Mark block in use.
                fs().log_write(bp);
                brelease(&mut *bp);
                bzero(dev as i32, (b + bi) as i32);
                return b + bi;
            }
            bi += 1
        }
        brelease(&mut *bp);
        b += BPB
    }
    panic!("balloc: out of blocks");
}

/// Free a disk block.
unsafe fn bfree(dev: i32, b: u32) {
    let mut bp: *mut Buf = Buf::read(dev as u32, fs().superblock.bblock(b));
    let bi: i32 = b.wrapping_rem(BPB) as i32;
    let m: i32 = (1) << (bi % 8);
    assert_ne!(
        (*bp).inner.data[(bi / 8) as usize] as i32 & m,
        0,
        "freeing free block"
    );
    (*bp).inner.data[(bi / 8) as usize] = ((*bp).inner.data[(bi / 8) as usize] as i32 & !m) as u8;
    fs().log_write(bp);
    brelease(&mut *bp);
}

/// Find the inode with number inum on device dev
/// and return the in-memory copy. Does not lock
/// the inode and does not read it from disk.
unsafe fn iget(dev: u32, inum: u32) -> *mut Inode {
    let mut inode = ICACHE.lock();

    // Is the inode already cached?
    let mut empty: *mut Inode = ptr::null_mut();
    for ip in &mut inode.deref_mut()[..] {
        if (*ip).ref_0 > 0 && (*ip).dev == dev && (*ip).inum == inum {
            (*ip).ref_0 += 1;
            return ip;
        }
        if empty.is_null() && (*ip).ref_0 == 0 {
            // Remember empty slot.
            empty = ip
        }
    }

    // Recycle an inode cache entry.
    assert!(!empty.is_null(), "iget: no inodes");
    let ip = empty;
    (*ip).dev = dev;
    (*ip).inum = inum;
    (*ip).ref_0 = 1;
    (*ip).inner.get_mut_unchecked().valid = false;
    ip
}
