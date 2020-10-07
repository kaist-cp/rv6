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
    bio::brelease,
    buf::Buf,
    file::{Inode, InodeGuard, InodeInner},
    log::log_write,
    param::{NINODE, ROOTDEV},
    proc::{either_copyin, either_copyout},
    sleeplock::SleeplockWIP,
    spinlock::Spinlock,
    stat::{Stat, T_DIR},
};
use core::{mem, ops::DerefMut, ptr};

mod path;
pub use path::{FileName, Path};

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

static mut ICACHE: Spinlock<[Inode; NINODE]> = Spinlock::new("ICACHE", [Inode::zeroed(); NINODE]);

impl InodeGuard<'_> {
    /// Unlock the given inode.
    pub unsafe fn unlock(self) {
        if (*self.ptr).ref_0 < 1 {
            panic!("Inode::unlock");
        }
        drop(self.guard);
    }

    /// Common idiom: unlock, then put.
    pub unsafe fn unlockput(self) {
        let ptr: *mut Inode = self.ptr;
        self.unlock();
        (*ptr).put();
    }

    /// Copy stat information from inode.
    /// Caller must hold ip->lock.
    pub unsafe fn stati(&self, st: &mut Stat) {
        (*st).dev = (*self.ptr).dev as i32;
        (*st).ino = (*self.ptr).inum;
        (*st).typ = self.guard.typ;
        (*st).nlink = self.guard.nlink;
        (*st).size = self.guard.size as usize;
    }

    // Directories
    /// Write a new directory entry (name, inum) into the directory dp.
    pub unsafe fn dirlink(&mut self, name: &FileName, inum: u32) -> Result<(), ()> {
        let mut de: Dirent = Default::default();

        // Check that name is not present.
        if let Ok((ip, _)) = self.dirlookup(name) {
            (*ip).put();
            return Err(());
        };

        // Look for an empty Dirent.
        let mut off: i32 = 0;
        while (off as u32) < self.guard.size {
            let bytes_read = self.read(
                0,
                &mut de as *mut Dirent as usize,
                off as u32,
                mem::size_of::<Dirent>() as u32,
            );
            assert!(
                !bytes_read.map_or(true, |v| v != mem::size_of::<Dirent>()),
                "dirlink read"
            );
            if de.inum as i32 == 0 {
                break;
            }
            off = (off as usize).wrapping_add(mem::size_of::<Dirent>()) as i32
        }
        de.inum = inum as u16;
        de.set_name(name);
        let bytes_write = self.write(
            0,
            &mut de as *mut Dirent as usize,
            off as u32,
            mem::size_of::<Dirent>() as u32,
        );
        assert!(
            !bytes_write.map_or(true, |v| v != mem::size_of::<Dirent>()),
            "dirlink"
        );
        Ok(())
    }

    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    /// Caller must hold self->lock.
    pub unsafe fn update(&mut self) {
        let bp: *mut Buf = Buf::read((*self.ptr).dev, SB.iblock((*self.ptr).inum));
        let mut dip: *mut Dinode = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
            .add(((*self.ptr).inum as usize).wrapping_rem(IPB));
        (*dip).typ = self.guard.typ;
        (*dip).major = self.guard.major as i16;
        (*dip).minor = self.guard.minor as i16;
        (*dip).nlink = self.guard.nlink;
        (*dip).size = self.guard.size;
        ptr::copy(
            self.guard.addrs.as_mut_ptr() as *const libc::CVoid,
            (*dip).addrs.as_mut_ptr() as *mut libc::CVoid,
            mem::size_of::<[u32; 13]>(),
        );
        log_write(bp);
        brelease(&mut *bp);
    }

    /// Truncate inode (discard contents).
    /// Only called when the inode has no links
    /// to it (no directory entries referring to it)
    /// and has no in-memory reference to it (is
    /// not an open file or current directory).
    unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if self.guard.addrs[i] != 0 {
                bfree((*self.ptr).dev as i32, self.guard.addrs[i]);
                self.guard.addrs[i] = 0
            }
        }
        if self.guard.addrs[NDIRECT] != 0 {
            let bp = Buf::read((*self.ptr).dev, self.guard.addrs[NDIRECT]);
            let a = (*bp).inner.data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.add(j) != 0 {
                    bfree((*self.ptr).dev as i32, *a.add(j));
                }
            }
            brelease(&mut *bp);
            bfree((*self.ptr).dev as i32, self.guard.addrs[NDIRECT]);
            self.guard.addrs[NDIRECT] = 0
        }
        self.guard.size = 0;
        self.update();
    }

    /// Read data from inode.
    /// Caller must hold self->lock.
    /// If user_dst==1, then dst is a user virtual address;
    /// otherwise, dst is a kernel address.
    pub unsafe fn read(
        &mut self,
        user_dst: i32,
        mut dst: usize,
        mut off: u32,
        mut n: u32,
    ) -> Result<usize, ()> {
        if off > self.guard.size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) > self.guard.size {
            n = self.guard.size.wrapping_sub(off)
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = Buf::read(
                (*self.ptr).dev,
                self.bmap((off as usize).wrapping_div(BSIZE)),
            );
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyout(
                user_dst,
                dst,
                (*bp)
                    .inner
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::CVoid,
                m as usize,
            )
            .is_err()
            {
                brelease(&mut *bp);
                break;
            } else {
                brelease(&mut *bp);
                tot = tot.wrapping_add(m);
                off = off.wrapping_add(m);
                dst = dst.wrapping_add(m as usize)
            }
        }
        Ok(n as usize)
    }

    /// Write data to inode.
    /// Caller must hold self->lock.
    /// If user_src==1, then src is a user virtual address;
    /// otherwise, src is a kernel address.
    pub unsafe fn write(
        &mut self,
        user_src: i32,
        mut src: usize,
        mut off: u32,
        n: u32,
    ) -> Result<usize, ()> {
        if off > self.guard.size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) as usize > MAXFILE.wrapping_mul(BSIZE) {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = Buf::read(
                (*self.ptr).dev,
                self.bmap((off as usize).wrapping_div(BSIZE)),
            );
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyin(
                (*bp)
                    .inner
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::CVoid,
                user_src,
                src,
                m as usize,
            )
            .is_err()
            {
                brelease(&mut *bp);
                break;
            } else {
                log_write(bp);
                brelease(&mut *bp);
                tot = tot.wrapping_add(m);
                off = off.wrapping_add(m);
                src = src.wrapping_add(m as usize)
            }
        }
        if n > 0 {
            if off > self.guard.size {
                self.guard.size = off
            }
            // write the i-node back to disk even if the size didn't change
            // because the loop above might have called bmap() and added a new
            // block to self->addrs[].
            (*self).update();
        }
        Ok(n as usize)
    }

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub unsafe fn dirlookup(&mut self, name: &FileName) -> Result<(*mut Inode, u32), ()> {
        let mut de: Dirent = Default::default();
        if self.guard.typ != T_DIR {
            panic!("dirlookup not DIR");
        }
        for off in (0..self.guard.size).step_by(mem::size_of::<Dirent>()) {
            let bytes_read = self.read(
                0,
                &mut de as *mut Dirent as usize,
                off,
                mem::size_of::<Dirent>() as u32,
            );
            assert!(
                !bytes_read.map_or(true, |v| v != mem::size_of::<Dirent>()),
                "dirlookup read"
            );
            if de.inum as i32 != 0 && name == de.get_name() {
                // entry matches path element
                return Ok((iget((*self.ptr).dev, de.inum as u32), off));
            }
        }
        Err(())
    }

    /// Inode content
    ///
    /// The content (data) associated with each inode is stored
    /// in blocks on the disk. The first NDIRECT block numbers
    /// are listed in self->addrs[].  The next NINDIRECT blocks are
    /// listed in block self->addrs[NDIRECT].
    /// Return the disk block address of the nth block in inode self.
    /// If there is no such block, bmap allocates one.
    unsafe fn bmap(&mut self, mut bn: usize) -> u32 {
        let mut addr: u32;
        if bn < NDIRECT {
            addr = self.guard.addrs[bn];
            if addr == 0 {
                addr = balloc((*self.ptr).dev);
                self.guard.addrs[bn] = addr
            }
            return addr;
        }
        bn = (bn).wrapping_sub(NDIRECT);
        if bn < NINDIRECT {
            // Load indirect block, allocating if necessary.
            addr = self.guard.addrs[NDIRECT];
            if addr == 0 {
                addr = balloc((*self.ptr).dev);
                self.guard.addrs[NDIRECT] = addr
            }
            let bp: *mut Buf = Buf::read((*self.ptr).dev, addr);
            let a: *mut u32 = (*bp).inner.data.as_mut_ptr() as *mut u32;
            addr = *a.add(bn);
            if addr == 0 {
                addr = balloc((*self.ptr).dev);
                *a.add(bn) = addr;
                log_write(bp);
            }
            brelease(&mut *bp);
            return addr;
        }
        panic!("bmap: out of range");
    }
}

impl Inode {
    /// Increment reference count for ip.
    /// Returns ip to enable ip = idup(ip1) idiom.
    pub unsafe fn idup(&mut self) -> *mut Self {
        let _inode = ICACHE.lock();
        self.ref_0 += 1;
        self
    }

    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    // lock() receives `ptr` because usertest halts at `fourfiles` when `ptr` isn't given.
    pub unsafe fn lock(&mut self, ptr: *mut Inode) -> InodeGuard<'_> {
        if (*self).ref_0 < 1 {
            panic!("Inode::lock");
        }
        let mut guard = (*self).inner.lock();
        if !self.valid {
            let bp: *mut Buf = Buf::read((*self).dev, SB.iblock(self.inum));
            let dip: *mut Dinode = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
                .add((self.inum as usize).wrapping_rem(IPB));
            guard.typ = (*dip).typ;
            guard.major = (*dip).major as u16;
            guard.minor = (*dip).minor as u16;
            guard.nlink = (*dip).nlink;
            guard.size = (*dip).size;
            ptr::copy(
                (*dip).addrs.as_mut_ptr() as *const libc::CVoid,
                guard.addrs.as_mut_ptr() as *mut libc::CVoid,
                mem::size_of::<[u32; 13]>(),
            );
            brelease(&mut *bp);
            self.valid = true;
            if guard.typ == 0 {
                panic!("Inode::lock: no type");
            }
        };
        InodeGuard { guard, ptr }
    }

    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    pub unsafe fn put(&mut self) {
        let mut inode = ICACHE.lock();

        if (*self).ref_0 == 1 && (*self).valid {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let ptr: *mut Inode = self;
            let mut ip = InodeGuard {
                guard: (*self).inner.lock(),
                ptr,
            };
            if ip.guard.nlink != 0 {
                self.ref_0 -= 1;
                return;
            }

            drop(inode);

            ip.itrunc();
            ip.guard.typ = 0;
            ip.update();
            self.valid = false;

            drop(ip.guard);

            inode = ICACHE.lock();
        }
        (*self).ref_0 -= 1;
        drop(inode);
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by  giving it type type.
    /// Returns an unlocked but allocated and referenced inode.
    pub unsafe fn alloc(dev: u32, typ: i16) -> *mut Inode {
        for inum in 1..SB.ninodes {
            let bp = Buf::read(dev, SB.iblock(inum));
            let dip = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
                .add((inum as usize).wrapping_rem(IPB));

            // a free inode
            if (*dip).typ == 0 {
                ptr::write_bytes(dip, 0, 1);
                (*dip).typ = typ;

                // mark it allocated on the disk
                log_write(bp);
                brelease(&mut *bp);
                return iget(dev, inum);
            }
            brelease(&mut *bp);
        }
        panic!("Inode::alloc: no inodes");
    }

    pub const fn zeroed() -> Self {
        // TODO: transient measure
        Self {
            dev: 0,
            inum: 0,
            ref_0: 0,
            valid: false,
            inner: SleeplockWIP::new(
                "inode",
                InodeInner {
                    typ: 0,
                    major: 0,
                    minor: 0,
                    nlink: 0,
                    size: 0,
                    addrs: [0; 13],
                },
            ),
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
    unsafe fn read(&mut self, dev: i32) {
        let bp: *mut Buf = Buf::read(dev as u32, 1);
        ptr::copy(
            (*bp).inner.data.as_mut_ptr() as *const libc::CVoid,
            self as *mut Superblock as *mut libc::CVoid,
            mem::size_of::<Superblock>(),
        );
        brelease(&mut *bp);
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
const BPB: u32 = BSIZE.wrapping_mul(8) as u32;

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// there should be one superblock per disk device, but we run with
/// only one device
static mut SB: Superblock = Superblock::zeroed();

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
    let bp: *mut Buf = Buf::read(dev as u32, bno as u32);
    ptr::write_bytes((*bp).inner.data.as_mut_ptr(), 0, BSIZE);
    log_write(bp);
    brelease(&mut *bp);
}

/// Blocks.
/// Allocate a zeroed disk block.
unsafe fn balloc(dev: u32) -> u32 {
    let mut b: u32 = 0;
    let mut bi: u32 = 0;
    while b < SB.size {
        let mut bp: *mut Buf = Buf::read(dev, SB.bblock(b));
        while bi < BPB && (b + bi) < SB.size {
            let m = (1) << (bi % 8);
            if (*bp).inner.data[(bi / 8) as usize] as i32 & m == 0 {
                // Is block free?
                (*bp).inner.data[(bi / 8) as usize] =
                    ((*bp).inner.data[(bi / 8) as usize] as i32 | m) as u8; // Mark block in use.
                log_write(bp);
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
    let mut bp: *mut Buf = Buf::read(dev as u32, SB.bblock(b));
    let bi: i32 = b.wrapping_rem(BPB) as i32;
    let m: i32 = (1) << (bi % 8);
    if (*bp).inner.data[(bi / 8) as usize] as i32 & m == 0 {
        panic!("freeing free block");
    }
    (*bp).inner.data[(bi / 8) as usize] = ((*bp).inner.data[(bi / 8) as usize] as i32 & !m) as u8;
    log_write(bp);
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
    if empty.is_null() {
        panic!("iget: no inodes");
    }
    let ip = empty;
    (*ip).dev = dev;
    (*ip).inum = inum;
    (*ip).ref_0 = 1;
    (*ip).valid = false;
    ip
}
