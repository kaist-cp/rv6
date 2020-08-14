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
use crate::libc;
use crate::{
    bio::bread,
    buf::Buf,
    file::Inode,
    log::log_write,
    param::{NINODE, ROOTDEV},
    proc::{either_copyin, either_copyout, myproc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::{Stat, T_DIR},
    string::{strncmp, strncpy},
};
use core::mem;
use core::{ops::DerefMut, ptr};

pub const FD_DEVICE: u32 = 3;
pub const FD_INODE: u32 = 2;
pub const FD_PIPE: u32 = 1;
pub const FD_NONE: u32 = 0;

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

#[derive(Default, Copy, Clone)]
pub struct Dirent {
    pub inum: u16,
    name: [u8; DIRSIZ],
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
    major: i16,

    /// Minor device number (T_DEVICE only)
    minor: i16,

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
/// SB.startinode. Each inode has a number, indicating its
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
struct Icache {
    inode: Spinlock<[Inode; NINODE]>,
}

impl Icache {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            inode: Spinlock::new("ICACHE", [Inode::zeroed(); NINODE]),
        }
    }
}

impl Inode {
    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    /// Caller must hold ip->lock.
    pub unsafe fn update(&mut self) {
        let bp: *mut Buf = bread(self.dev, SB.iblock(self.inum));
        let mut dip: *mut Dinode =
            ((*bp).data.as_mut_ptr() as *mut Dinode).add((self.inum as usize).wrapping_rem(IPB));
        (*dip).typ = self.typ;
        (*dip).major = self.major;
        (*dip).minor = self.minor;
        (*dip).nlink = self.nlink;
        (*dip).size = self.size;
        ptr::copy(
            self.addrs.as_mut_ptr() as *const libc::CVoid,
            (*dip).addrs.as_mut_ptr() as *mut libc::CVoid,
            ::core::mem::size_of::<[u32; 13]>(),
        );
        log_write(bp);
        (*bp).release();
    }

    /// Increment reference count for ip.
    /// Returns ip to enable ip = idup(ip1) idiom.
    pub unsafe fn idup(&mut self) -> *mut Self {
        ICACHE.inode.lock();
        self.ref_0 += 1;
        self
    }

    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub unsafe fn lock(&mut self) {
        if (self as *mut Inode).is_null() || (*self).ref_0 < 1 {
            panic!("Inode::lock");
        }
        (*self).lock.acquire();
        if (*self).valid == 0 {
            let bp: *mut Buf = bread((*self).dev, SB.iblock((*self).inum));
            let dip: *mut Dinode = ((*bp).data.as_mut_ptr() as *mut Dinode)
                .add(((*self).inum as usize).wrapping_rem(IPB));
            (*self).typ = (*dip).typ;
            (*self).major = (*dip).major;
            (*self).minor = (*dip).minor;
            (*self).nlink = (*dip).nlink;
            (*self).size = (*dip).size;
            ptr::copy(
                (*dip).addrs.as_mut_ptr() as *const libc::CVoid,
                (*self).addrs.as_mut_ptr() as *mut libc::CVoid,
                ::core::mem::size_of::<[u32; 13]>(),
            );
            (*bp).release();
            (*self).valid = 1;
            if (*self).typ as i32 == 0 {
                panic!("Inode::lock: no type");
            }
        };
    }

    /// Unlock the given inode.
    pub unsafe fn unlock(&mut self) {
        if (self as *mut Inode).is_null() || (*self).lock.holding() == 0 || (*self).ref_0 < 1 {
            panic!("Inode::unlock");
        }
        (*self).lock.release();
    }

    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    pub unsafe fn put(&mut self) {
        let inode = ICACHE.inode.lock();

        let _inode;
        if (*self).ref_0 == 1 && (*self).valid != 0 && (*self).nlink as i32 == 0 {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            (*self).lock.acquire();

            drop(inode);

            self.itrunc();
            (*self).typ = 0;
            (*self).update();
            (*self).valid = 0;

            (*self).lock.release();

            _inode = ICACHE.inode.lock();
        }
        (*self).ref_0 -= 1;
    }

    /// Common idiom: unlock, then put.
    pub unsafe fn unlockput(&mut self) {
        self.unlock();
        self.put();
    }

    /// Inode content
    ///
    /// The content (data) associated with each inode is stored
    /// in blocks on the disk. The first NDIRECT block numbers
    /// are listed in self->addrs[].  The next NINDIRECT blocks are
    /// listed in block self->addrs[NDIRECT].
    /// Return the disk block address of the nth block in inode self.
    /// If there is no such block, bmap allocates one.
    unsafe fn bmap(&mut self, mut bn: u32) -> u32 {
        let mut addr: u32;
        if bn < NDIRECT as u32 {
            addr = (*self).addrs[bn as usize];
            if addr == 0 {
                addr = balloc((*self).dev);
                (*self).addrs[bn as usize] = addr
            }
            return addr;
        }
        bn = (bn).wrapping_sub(NDIRECT as u32);
        if (bn as usize) < NINDIRECT {
            // Load indirect block, allocating if necessary.
            addr = (*self).addrs[NDIRECT];
            if addr == 0 {
                addr = balloc((*self).dev);
                (*self).addrs[NDIRECT] = addr
            }
            let bp: *mut Buf = bread((*self).dev, addr);
            let a: *mut u32 = (*bp).data.as_mut_ptr() as *mut u32;
            addr = *a.offset(bn as isize);
            if addr == 0 {
                addr = balloc((*self).dev);
                *a.offset(bn as isize) = addr;
                log_write(bp);
            }
            (*bp).release();
            return addr;
        }
        panic!("bmap: out of range");
    }

    /// Truncate inode (discard contents).
    /// Only called when the inode has no links
    /// to it (no directory entries referring to it)
    /// and has no in-memory reference to it (is
    /// not an open file or current directory).
    unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if (*self).addrs[i] != 0 {
                bfree((*self).dev as i32, (*self).addrs[i as usize]);
                (*self).addrs[i] = 0
            }
        }
        if (*self).addrs[NDIRECT] != 0 {
            let bp = bread((*self).dev, (*self).addrs[NDIRECT]);
            let a = (*bp).data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.add(j) != 0 {
                    bfree((*self).dev as i32, *a.add(j));
                }
            }
            (*bp).release();
            bfree((*self).dev as i32, (*self).addrs[NDIRECT]);
            (*self).addrs[NDIRECT] = 0
        }
        (*self).size = 0;
        (*self).update();
    }

    /// Read data from inode.
    /// Caller must hold self->lock.
    /// If user_dst==1, then dst is a user virtual address;
    /// otherwise, dst is a kernel address.
    pub unsafe fn read(&mut self, user_dst: i32, mut dst: usize, mut off: u32, mut n: u32) -> i32 {
        if off > (*self).size || off.wrapping_add(n) < off {
            return -1;
        }
        if off.wrapping_add(n) > (*self).size {
            n = (*self).size.wrapping_sub(off)
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = bread((*self).dev, self.bmap(off.wrapping_div(BSIZE as u32)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyout(
                user_dst,
                dst,
                (*bp)
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::CVoid,
                m as usize,
            ) == -1
            {
                (*bp).release();
                break;
            } else {
                (*bp).release();
                tot = (tot as u32).wrapping_add(m) as u32 as u32;
                off = (off as u32).wrapping_add(m) as u32 as u32;
                dst = (dst as usize).wrapping_add(m as usize) as usize as usize
            }
        }
        n as i32
    }

    /// Write data to inode.
    /// Caller must hold self->lock.
    /// If user_src==1, then src is a user virtual address;
    /// otherwise, src is a kernel address.
    pub unsafe fn write(&mut self, user_src: i32, mut src: usize, mut off: u32, n: u32) -> i32 {
        if off > (*self).size || off.wrapping_add(n) < off {
            return -1;
        }
        if off.wrapping_add(n) as usize > MAXFILE.wrapping_mul(BSIZE) {
            return -1;
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = bread((*self).dev, self.bmap(off.wrapping_div(BSIZE as u32)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyin(
                (*bp)
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::CVoid,
                user_src,
                src,
                m as usize,
            ) == -1
            {
                (*bp).release();
                break;
            } else {
                log_write(bp);
                (*bp).release();
                tot = (tot as u32).wrapping_add(m) as u32 as u32;
                off = (off as u32).wrapping_add(m) as u32 as u32;
                src = (src as usize).wrapping_add(m as usize) as usize as usize
            }
        }
        if n > 0 {
            if off > (*self).size {
                (*self).size = off
            }
            // write the i-node back to disk even if the size didn't change
            // because the loop above might have called bmap() and added a new
            // block to self->addrs[].
            (*self).update();
        }
        n as i32
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by  giving it type type.
    /// Returns an unlocked but allocated and referenced inode.
    pub unsafe fn alloc(dev: u32, typ: i16) -> *mut Inode {
        for inum in 1..SB.ninodes {
            let bp = bread(dev, SB.iblock(inum));
            let dip =
                ((*bp).data.as_mut_ptr() as *mut Dinode).add((inum as usize).wrapping_rem(IPB));

            // a free inode
            if (*dip).typ as i32 == 0 {
                ptr::write_bytes(dip, 0, 1);
                (*dip).typ = typ;

                // mark it allocated on the disk
                log_write(bp);
                (*bp).release();
                return iget(dev, inum as u32);
            }
            (*bp).release();
        }
        panic!("Inode::alloc: no inodes");
    }

    pub const fn zeroed() -> Self {
        // TODO: transient measure
        Self {
            dev: 0,
            inum: 0,
            ref_0: 0,
            lock: Sleeplock::zeroed(),
            valid: 0,
            typ: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            size: 0,
            addrs: [0; 13],
        }
    }
}

/// root i-number
pub const ROOTINO: u32 = 1;

/// block size
pub const BSIZE: usize = 1024;
pub const FSMAGIC: u32 = 0x10203040;
pub const NDIRECT: usize = 12;

pub const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
pub const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

/// Inodes per block.
pub const IPB: usize = BSIZE.wrapping_div(mem::size_of::<Dinode>());

impl Superblock {
    /// Block containing inode i
    const fn iblock(self, i: u32) -> u32 {
        i.wrapping_div(IPB as u32)
            .wrapping_add(self.inodestart as u32)
    }

    /// Block of free map containing bit for block b
    const fn bblock(self, b: u32) -> u32 {
        b.wrapping_div(BPB).wrapping_add(self.bmapstart)
    }

    /// Read the super block.
    unsafe fn read(&mut self, dev: i32) {
        let bp: *mut Buf = bread(dev as u32, 1);
        ptr::copy(
            (*bp).data.as_mut_ptr() as *const libc::CVoid,
            self as *mut Superblock as *mut libc::CVoid,
            ::core::mem::size_of::<Superblock>(),
        );
        (*bp).release();
    }

    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            size: 0,
            nblocks: 0,
            ninodes: 0,
            nlog: 0,
            logstart: 0,
            inodestart: 0,
            bmapstart: 0,
        }
    }
}

/// Bitmap bits per block
pub const BPB: u32 = BSIZE.wrapping_mul(8) as u32;

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// there should be one superblock per disk device, but we run with
/// only one device
pub static mut SB: Superblock = Superblock::zeroed();

/// Init fs
pub unsafe fn fsinit(dev: i32) {
    SB.read(dev);
    if SB.magic != FSMAGIC {
        panic!("invalid file system");
    }
    SB.initlog(dev);
}

/// Zero a block.
unsafe fn bzero(dev: i32, bno: i32) {
    let bp: *mut Buf = bread(dev as u32, bno as u32);
    ptr::write_bytes((*bp).data.as_mut_ptr(), 0, BSIZE);
    log_write(bp);
    (*bp).release();
}

/// Blocks.
/// Allocate a zeroed disk block.
unsafe fn balloc(dev: u32) -> u32 {
    let mut b: u32 = 0;
    let mut bi: u32 = 0;
    while b < SB.size {
        let mut bp: *mut Buf = bread(dev, SB.bblock(b));
        while bi < BPB && (b + bi) < SB.size {
            let m = (1) << (bi % 8);
            if (*bp).data[(bi / 8) as usize] as i32 & m == 0 {
                // Is block free?
                (*bp).data[(bi / 8) as usize] = ((*bp).data[(bi / 8) as usize] as i32 | m) as u8; // Mark block in use.
                log_write(bp);
                (*bp).release();
                bzero(dev as i32, (b + bi) as i32);
                return b + bi;
            }
            bi += 1
        }
        (*bp).release();
        b += BPB
    }
    panic!("balloc: out of blocks");
}

/// Free a disk block.
unsafe fn bfree(dev: i32, b: u32) {
    let mut bp: *mut Buf = bread(dev as u32, SB.bblock(b));
    let bi: i32 = b.wrapping_rem(BPB) as i32;
    let m: i32 = (1) << (bi % 8);
    if (*bp).data[(bi / 8) as usize] as i32 & m == 0 {
        panic!("freeing free block");
    }
    (*bp).data[(bi / 8) as usize] = ((*bp).data[(bi / 8) as usize] as i32 & !m) as u8;
    log_write(bp);
    (*bp).release();
}

static mut ICACHE: Icache = Icache::zeroed();

pub unsafe fn iinit() {
    let mut inode = ICACHE.inode.lock();
    for i in 0..NINODE {
        (*inode.as_mut_ptr().add(i)).lock.initlock("inode");
    }
}

/// Find the inode with number inum on device dev
/// and return the in-memory copy. Does not lock
/// the inode and does not read it from disk.
unsafe fn iget(dev: u32, inum: u32) -> *mut Inode {
    let mut inode = ICACHE.inode.lock();

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
    if empty.is_null() {
        panic!("iget: no inodes");
    }
    let ip = empty;
    (*ip).dev = dev;
    (*ip).inum = inum;
    (*ip).ref_0 = 1;
    (*ip).valid = 0;
    ip
}

/// Copy stat information from inode.
/// Caller must hold ip->lock.
pub unsafe fn stati(ip: *mut Inode, mut st: *mut Stat) {
    (*st).dev = (*ip).dev as i32;
    (*st).ino = (*ip).inum;
    (*st).typ = (*ip).typ;
    (*st).nlink = (*ip).nlink;
    (*st).size = (*ip).size as usize;
}

/// Directories
pub unsafe fn namecmp(s: *const u8, t: *const u8) -> i32 {
    strncmp(s, t, DIRSIZ as u32)
}

/// Look for a directory entry in a directory.
/// If found, set *poff to byte offset of entry.
pub unsafe fn dirlookup(dp: *mut Inode, name: *mut u8, poff: *mut u32) -> *mut Inode {
    let mut off: u32 = 0;
    let mut de: Dirent = Default::default();
    if (*dp).typ as i32 != T_DIR {
        panic!("dirlookup not DIR");
    }
    while off < (*dp).size {
        if (*dp).read(
            0,
            &mut de as *mut Dirent as usize,
            off,
            ::core::mem::size_of::<Dirent>() as u32,
        ) as usize
            != ::core::mem::size_of::<Dirent>()
        {
            panic!("dirlookup read");
        }
        if de.inum as i32 != 0 && namecmp(name, de.name.as_mut_ptr()) == 0 {
            // entry matches path element
            if !poff.is_null() {
                *poff = off
            }
            return iget((*dp).dev, de.inum as u32);
        }
        off = (off as usize).wrapping_add(::core::mem::size_of::<Dirent>()) as u32 as u32
    }
    ptr::null_mut()
}

/// Write a new directory entry (name, inum) into the directory dp.
pub unsafe fn dirlink(dp: *mut Inode, name: *mut u8, inum: u32) -> i32 {
    let mut de: Dirent = Default::default();

    // Check that name is not present.
    let ip: *mut Inode = dirlookup(dp, name, ptr::null_mut());
    if !ip.is_null() {
        (*ip).put();
        return -1;
    }

    // Look for an empty Dirent.
    let mut off: i32 = 0;
    while (off as u32) < (*dp).size {
        if (*dp).read(
            0,
            &mut de as *mut Dirent as usize,
            off as u32,
            ::core::mem::size_of::<Dirent>() as u32,
        ) as usize
            != ::core::mem::size_of::<Dirent>()
        {
            panic!("dirlink read");
        }
        if de.inum as i32 == 0 {
            break;
        }
        off = (off as usize).wrapping_add(::core::mem::size_of::<Dirent>()) as i32
    }
    strncpy(de.name.as_mut_ptr(), name, DIRSIZ as i32);
    de.inum = inum as u16;
    if (*dp).write(
        0,
        &mut de as *mut Dirent as usize,
        off as u32,
        ::core::mem::size_of::<Dirent>() as u32,
    ) as usize
        != ::core::mem::size_of::<Dirent>()
    {
        panic!("dirlink");
    }
    0
}

/// Paths
///
/// Copy the next path element from path into name.
/// Return a pointer to the element following the copied one.
/// The returned path has no leading slashes,
/// so the caller can check *path=='\0' to see if the name is the last one.
/// If no name to remove, return 0.
///
/// Examples:
///   skipelem("a/bb/c", name) = "bb/c", setting name = "a"
///   skipelem("///a//bb", name) = "bb", setting name = "a"
///   skipelem("a", name) = "", setting name = "a"
///   skipelem("", name) = skipelem("////", name) = 0
unsafe fn skipelem(mut path: *mut u8, name: *mut u8) -> *mut u8 {
    while *path as i32 == '/' as i32 {
        path = path.offset(1)
    }
    if *path as i32 == 0 {
        return ptr::null_mut();
    }
    let s: *mut u8 = path;
    while *path as i32 != '/' as i32 && *path as i32 != 0 {
        path = path.offset(1)
    }
    let len: i32 = path.offset_from(s) as i64 as i32;
    if len >= DIRSIZ as i32 {
        ptr::copy(s as *const libc::CVoid, name as *mut libc::CVoid, DIRSIZ);
    } else {
        ptr::copy(
            s as *const libc::CVoid,
            name as *mut libc::CVoid,
            len as usize,
        );
        *name.offset(len as isize) = 0
    }
    while *path as i32 == '/' as i32 {
        path = path.offset(1)
    }
    path
}

/// Look up and return the inode for a path name.
/// If parent != 0, return the inode for the parent and copy the final
/// path element into name, which must have room for DIRSIZ bytes.
/// Must be called inside a transaction since it calls Inode::put().
unsafe fn namex(mut path: *mut u8, nameiparent_0: i32, name: *mut u8) -> *mut Inode {
    let mut ip: *mut Inode;

    if *path as i32 == '/' as i32 {
        ip = iget(ROOTDEV as u32, ROOTINO)
    } else {
        ip = (*(*myproc()).cwd).idup()
    }
    loop {
        path = skipelem(path, name);
        if path.is_null() {
            break;
        }
        (*ip).lock();
        if (*ip).typ as i32 != T_DIR {
            (*ip).unlockput();
            return ptr::null_mut();
        }
        if nameiparent_0 != 0 && *path as i32 == '\u{0}' as i32 {
            // Stop one level early.
            (*ip).unlock();
            return ip;
        }
        let next: *mut Inode = dirlookup(ip, name, ptr::null_mut());
        if next.is_null() {
            (*ip).unlockput();
            return ptr::null_mut();
        }
        (*ip).unlockput();
        ip = next
    }
    if nameiparent_0 != 0 {
        (*ip).put();
        return ptr::null_mut();
    }
    ip
}

pub unsafe fn namei(path: *mut u8) -> *mut Inode {
    let mut name: [u8; DIRSIZ] = [0; DIRSIZ];
    namex(path, 0, name.as_mut_ptr())
}

pub unsafe fn nameiparent(path: *mut u8, name: *mut u8) -> *mut Inode {
    namex(path, 1, name)
}
