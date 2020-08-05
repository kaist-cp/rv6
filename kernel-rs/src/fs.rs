// File system implementation.  Five layers:
//   + Blocks: allocator for raw disk blocks.
//   + Log: crash recovery for multi-step updates.
//   + Files: inode allocator, reading, writing, metadata.
//   + Directories: inode with special contents (list of other inodes!)
//   + Names: paths like /usr/rtm/xv6/fs.c for convenient naming.
//
// This file contains the low-level file system manipulation
// routines.  The (higher-level) system call implementations
// are in sysfile.c.

use crate::libc;
use crate::{
    bio::{bread, brelse},
    buf::Buf,
    file::Inode,
    log::{initlog, log_write},
    param::{NINODE, ROOTDEV},
    printf::panic,
    proc::{either_copyin, either_copyout, myproc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::{Stat, T_DIR},
    string::{strncmp, strncpy},
};
use core::mem;
use core::ptr;
pub const FD_DEVICE: u32 = 3;
pub const FD_INODE: u32 = 2;
pub const FD_PIPE: u32 = 1;
pub const FD_NONE: u32 = 0;

/// block size
/// Disk layout:
/// [ boot block | super block | log | inode blocks |
///                                          free bit map | data blocks]
///
/// mkfs computes the super block and builds an initial file system. The
/// super block describes the disk layout:
#[derive(Copy, Clone)]
pub struct Superblock {
    pub magic: u32,
    pub size: u32,
    pub nblocks: u32,
    pub ninodes: u32,
    pub nlog: u32,
    pub logstart: u32,
    pub inodestart: u32,
    pub bmapstart: u32,
}

#[derive(Default, Copy, Clone)]
pub struct Dirent {
    pub inum: u16,
    pub name: [libc::c_char; DIRSIZ],
}

/// On-disk inode structure
/// Both the kernel and user programs use this header file.
#[derive(Copy, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct Dinode {
    pub typ: i16,
    pub major: i16,
    pub minor: i16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; 13],
}

/// Inodes.
///
/// An inode describes a single unnamed file.
/// The inode disk structure holds metadata: the file's type,
/// its size, the number of links referring to it, and the
/// list of blocks holding the file's content.
///
/// The inodes are laid out sequentially on disk at
/// sb.startinode. Each inode has a number, indicating its
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
///   is non-zero. ialloc() allocates, and put() frees if
///   the reference and link counts have fallen to zero.
///
/// * Referencing in cache: an entry in the inode cache
///   is free if ip->ref is zero. Otherwise ip->ref tracks
///   the number of in-memory pointers to the entry (open
///   files and current directories). iget() finds or
///   creates a cache entry and increments its ref; put()
///   decrements ref.
///
/// * Valid: the information (type, size, &c) in an inode
///   cache entry is only correct when ip->valid is 1.
///   lock() reads the inode from
///   the disk and sets ip->valid, while put() clears
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
/// lock() is separate from iget() so that system calls can
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
/// The icache.lock spin-lock protects the allocation of icache
/// entries. Since ip->ref indicates whether an entry is free,
/// and ip->dev and ip->inum indicate which i-node an entry
/// holds, one must hold icache.lock while using any of those fields.
///
/// An ip->lock sleep-lock protects all ip-> fields other than ref,
/// dev, and inum.  One must hold ip->lock in order to
/// read or write that inode's ip->valid, ip->size, ip->type, &c.
#[derive(Copy, Clone)]
pub struct Icache {
    pub lock: Spinlock,
    pub inode: [Inode; 50],
}

impl Superblock {
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

impl Icache {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
            inode: [Inode::zeroed(); 50],
        }
    }
}

impl Inode {
    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    /// Caller must hold ip->lock.
    pub unsafe fn update(&mut self) {
        let mut bp: *mut Buf = ptr::null_mut();
        let mut dip: *mut Dinode = ptr::null_mut();
        bp = bread(self.dev, iblock(self.inum as i32, sb));
        dip = ((*bp).data.as_mut_ptr() as *mut Dinode)
            .offset((self.inum as u64).wrapping_rem(IPB as u64) as isize);
        (*dip).typ = self.typ;
        (*dip).major = self.major;
        (*dip).minor = self.minor;
        (*dip).nlink = self.nlink;
        (*dip).size = self.size;
        ptr::copy(
            self.addrs.as_mut_ptr() as *const libc::c_void,
            (*dip).addrs.as_mut_ptr() as *mut libc::c_void,
            ::core::mem::size_of::<[u32; 13]>(),
        );
        log_write(bp);
        brelse(bp);
    }

    /// Increment reference count for ip.
    /// Returns ip to enable ip = idup(ip1) idiom.
    pub unsafe fn idup(&mut self) -> *mut Self {
        icache.lock.acquire();
        self.ref_0 += 1;
        icache.lock.release();
        self
    }

    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub unsafe fn lock(&mut self) {
        let mut bp: *mut Buf = ptr::null_mut();
        let mut dip: *mut Dinode = ptr::null_mut();
        if (self as *mut Inode).is_null() || (*self).ref_0 < 1 as i32 {
            panic(b"ilock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        (*self).lock.acquire();
        if (*self).valid == 0 as i32 {
            bp = bread((*self).dev, iblock((*self).inum as i32, sb));
            dip = ((*bp).data.as_mut_ptr() as *mut Dinode)
                .offset(((*self).inum as u64).wrapping_rem(IPB as u64) as isize);
            (*self).typ = (*dip).typ;
            (*self).major = (*dip).major;
            (*self).minor = (*dip).minor;
            (*self).nlink = (*dip).nlink;
            (*self).size = (*dip).size;
            ptr::copy(
                (*dip).addrs.as_mut_ptr() as *const libc::c_void,
                (*self).addrs.as_mut_ptr() as *mut libc::c_void,
                ::core::mem::size_of::<[u32; 13]>(),
            );
            brelse(bp);
            (*self).valid = 1 as i32;
            if (*self).typ as i32 == 0 as i32 {
                panic(
                    b"ilock: no type\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                );
            }
        };
    }

    /// Unlock the given inode.
    pub unsafe fn unlock(&mut self) {
        if (self as *mut Inode).is_null() || (*self).lock.holding() == 0 || (*self).ref_0 < 1 as i32
        {
            panic(b"iunlock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        (*self).lock.release();
    }

    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to put() must be inside a transaction in
    /// case it has to free the inode.
    pub unsafe fn put(&mut self) {
        icache.lock.acquire();

        if (*self).ref_0 == 1 as i32 && (*self).valid != 0 && (*self).nlink as i32 == 0 as i32 {
            // inode has no links and no other references: truncate and free.
            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            (*self).lock.acquire();
            icache.lock.release();
            self.itrunc();
            (*self).typ = 0 as i32 as i16;
            (*self).update();
            (*self).valid = 0 as i32;
            (*self).lock.release();
            icache.lock.acquire();
        }
        (*self).ref_0 -= 1;
        icache.lock.release();
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
        let mut addr: u32 = 0;
        let mut a: *mut u32 = ptr::null_mut();
        let mut bp: *mut Buf = ptr::null_mut();
        if bn < NDIRECT as u32 {
            addr = (*self).addrs[bn as usize];
            if addr == 0 as i32 as u32 {
                addr = balloc((*self).dev);
                (*self).addrs[bn as usize] = addr
            }
            return addr;
        }
        bn = (bn as u32).wrapping_sub(NDIRECT as u32) as u32 as u32;
        if (bn as u64) < NINDIRECT as u64 {
            // Load indirect block, allocating if necessary.
            addr = (*self).addrs[NDIRECT as usize];
            if addr == 0 as i32 as u32 {
                addr = balloc((*self).dev);
                (*self).addrs[NDIRECT as usize] = addr
            }
            bp = bread((*self).dev, addr);
            a = (*bp).data.as_mut_ptr() as *mut u32;
            addr = *a.offset(bn as isize);
            if addr == 0 as i32 as u32 {
                addr = balloc((*self).dev);
                *a.offset(bn as isize) = addr;
                log_write(bp);
            }
            brelse(bp);
            return addr;
        }
        panic(b"bmap: out of range\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }

    /// File system implementation.  Five layers:
    ///   + Blocks: allocator for raw disk blocks.
    ///   + Log: crash recovery for multi-step updates.
    ///   + Files: inode allocator, reading, writing, metadata.
    ///   + Directories: inode with special contents (list of other inodes!)
    ///   + Names: paths like /usr/rtm/xv6/fs.c for convenient naming.
    ///
    /// This file contains the low-level file system manipulation
    /// routines.  The (higher-level) system call implementations
    /// are in sysfile.c.
    /// Truncate inode (discard contents).
    /// Only called when the inode has no links
    /// to it (no directory entries referring to it)
    /// and has no in-memory reference to it (is
    /// not an open file or current directory).
    unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if (*self).addrs[i as usize] != 0 {
                bfree((*self).dev as i32, (*self).addrs[i as usize]);
                (*self).addrs[i as usize] = 0 as i32 as u32
            }
        }
        if (*self).addrs[NDIRECT as usize] != 0 {
            let bp = bread((*self).dev, (*self).addrs[NDIRECT as usize]);
            let a = (*bp).data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.offset(j as isize) != 0 {
                    bfree((*self).dev as i32, *a.offset(j as isize));
                }
            }
            brelse(bp);
            bfree((*self).dev as i32, (*self).addrs[NDIRECT as usize]);
            (*self).addrs[NDIRECT as usize] = 0 as i32 as u32
        }
        (*self).size = 0 as i32 as u32;
        (*self).update();
    }

    /// Read data from inode.
    /// Caller must hold self->lock.
    /// If user_dst==1, then dst is a user virtual address;
    /// otherwise, dst is a kernel address.
    pub unsafe fn read(
        &mut self,
        mut user_dst: i32,
        mut dst: u64,
        mut off: u32,
        mut n: u32,
    ) -> i32 {
        let mut tot: u32 = 0;
        if off > (*self).size || off.wrapping_add(n) < off {
            return -1;
        }
        if off.wrapping_add(n) > (*self).size {
            n = (*self).size.wrapping_sub(off)
        }
        tot = 0 as u32;
        while tot < n {
            let bp = bread((*self).dev, self.bmap(off.wrapping_div(BSIZE as u32)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (1024 as i32 as u32).wrapping_sub(off.wrapping_rem(1024 as i32 as u32)),
            );
            if either_copyout(
                user_dst,
                dst,
                (*bp)
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::c_void,
                m as u64,
            ) == -(1 as i32)
            {
                brelse(bp);
                break;
            } else {
                brelse(bp);
                tot = (tot as u32).wrapping_add(m) as u32 as u32;
                off = (off as u32).wrapping_add(m) as u32 as u32;
                dst = (dst as u64).wrapping_add(m as u64) as u64 as u64
            }
        }
        n as i32
    }

    /// Write data to inode.
    /// Caller must hold self->lock.
    /// If user_src==1, then src is a user virtual address;
    /// otherwise, src is a kernel address.
    pub unsafe fn write(
        &mut self,
        mut user_src: i32,
        mut src: u64,
        mut off: u32,
        mut n: u32,
    ) -> i32 {
        let mut tot: u32 = 0;
        if off > (*self).size || off.wrapping_add(n) < off {
            return -1;
        }
        if off.wrapping_add(n) as u64 > MAXFILE.wrapping_mul(BSIZE) as u64 {
            return -1;
        }
        tot = 0 as i32 as u32;
        while tot < n {
            let bp = bread((*self).dev, self.bmap(off.wrapping_div(BSIZE as u32)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (1024 as i32 as u32).wrapping_sub(off.wrapping_rem(1024 as i32 as u32)),
            );
            if either_copyin(
                (*bp)
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize)
                    as *mut libc::c_void,
                user_src,
                src,
                m as u64,
            ) == -(1 as i32)
            {
                brelse(bp);
                break;
            } else {
                log_write(bp);
                brelse(bp);
                tot = (tot as u32).wrapping_add(m) as u32 as u32;
                off = (off as u32).wrapping_add(m) as u32 as u32;
                src = (src as u64).wrapping_add(m as u64) as u64 as u64
            }
        }
        if n > 0 as i32 as u32 {
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

    /// Look for a directory entry in a directory.
    /// If found, set *poff to byte offset of entry.
    pub unsafe fn dirlookup(
        &mut self,
        mut name: *mut libc::c_char,
        mut poff: *mut u32,
    ) -> *mut Inode {
        let mut off: u32 = 0;
        let mut de: Dirent = Default::default();
        if (*self).typ as i32 != T_DIR {
            panic(
                b"dirlookup not DIR\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
        }
        while off < (*self).size {
            if (*self).read(
                0 as i32,
                &mut de as *mut Dirent as u64,
                off,
                ::core::mem::size_of::<Dirent>() as u64 as u32,
            ) as u64
                != ::core::mem::size_of::<Dirent>() as u64
            {
                panic(
                    b"dirlookup read\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                );
            }
            if de.inum as i32 != 0 as i32 && namecmp(name, de.name.as_mut_ptr()) == 0 as i32 {
                // entry matches path element
                if !poff.is_null() {
                    *poff = off
                }
                return iget((*self).dev, de.inum as u32);
            }
            off = (off as u64).wrapping_add(::core::mem::size_of::<Dirent>() as u64) as u32 as u32
        }
        ptr::null_mut()
    }

    /// Write a new directory entry (name, inum) into the directory self.
    pub unsafe fn dirlink(&mut self, mut name: *mut libc::c_char, mut inum: u32) -> i32 {
        let mut off: i32 = 0;
        let mut de: Dirent = Default::default();
        let mut ip: *mut Inode = ptr::null_mut();

        // Check that name is not present.
        ip = self.dirlookup(name, ptr::null_mut());
        if !ip.is_null() {
            (*ip).put();
            return -(1 as i32);
        }

        // Look for an empty Dirent.
        off = 0;
        while (off as u32) < (*self).size {
            if (*self).read(
                0 as i32,
                &mut de as *mut Dirent as u64,
                off as u32,
                ::core::mem::size_of::<Dirent>() as u64 as u32,
            ) as u64
                != ::core::mem::size_of::<Dirent>() as u64
            {
                panic(b"dirlink read\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
            }
            if de.inum as i32 == 0 as i32 {
                break;
            }
            off = (off as u64).wrapping_add(::core::mem::size_of::<Dirent>() as u64) as i32 as i32
        }
        strncpy(de.name.as_mut_ptr(), name, DIRSIZ as i32);
        de.inum = inum as u16;
        if (*self).write(
            0 as i32,
            &mut de as *mut Dirent as u64,
            off as u32,
            ::core::mem::size_of::<Dirent>() as u64 as u32,
        ) as u64
            != ::core::mem::size_of::<Dirent>() as u64
        {
            panic(b"dirlink\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        0 as i32
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

/// On-disk file system format.
/// Both the kernel and user programs use this header file.
/// root i-number
pub const ROOTINO: i32 = 1;

/// block size
pub const BSIZE: i32 = 1024;
pub const FSMAGIC: i32 = 0x10203040;
pub const NDIRECT: i32 = 12;

pub const NINDIRECT: i32 = BSIZE.wrapping_div(mem::size_of::<u32>() as i32);
pub const MAXFILE: i32 = NDIRECT.wrapping_add(NINDIRECT);

/// Inodes per block.
pub const IPB: i32 = BSIZE.wrapping_div(mem::size_of::<Dinode>() as i32);

/// Block containing inode i
pub const fn iblock(i: i32, super_block: Superblock) -> u32 {
    i.wrapping_div(IPB)
        .wrapping_add(super_block.inodestart as i32) as u32
}

/// Block of free map containing bit for block b
pub const fn bblock(b: u32, super_block: Superblock) -> u32 {
    b.wrapping_div(BPB as u32)
        .wrapping_add(super_block.bmapstart)
}

/// Bitmap bits per block
pub const BPB: i32 = BSIZE * 8;

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// there should be one superblock per disk device, but we run with
/// only one device
pub static mut sb: Superblock = Superblock::zeroed();

/// Read the super block.
unsafe fn readsb(mut dev: i32, mut sb_0: *mut Superblock) {
    let mut bp: *mut Buf = ptr::null_mut();
    bp = bread(dev as u32, 1 as u32);
    ptr::copy(
        (*bp).data.as_mut_ptr() as *const libc::c_void,
        sb_0 as *mut libc::c_void,
        ::core::mem::size_of::<Superblock>(),
    );
    brelse(bp);
}

/// Init fs
pub unsafe fn fsinit(mut dev: i32) {
    readsb(dev, &mut sb);
    if sb.magic != FSMAGIC as u32 {
        panic(b"invalid file system\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    initlog(dev, &mut sb);
}

/// Zero a block.
unsafe fn bzero(mut dev: i32, mut bno: i32) {
    let mut bp: *mut Buf = ptr::null_mut();
    bp = bread(dev as u32, bno as u32);
    ptr::write_bytes((*bp).data.as_mut_ptr(), 0, BSIZE as usize);
    log_write(bp);
    brelse(bp);
}

/// Blocks.
/// Allocate a zeroed disk block.
unsafe fn balloc(mut dev: u32) -> u32 {
    let mut b: i32 = 0;
    let mut bi: i32 = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    bp = ptr::null_mut();
    while (b as u32) < sb.size {
        bp = bread(dev, bblock(b as u32, sb));
        while bi < BPB && ((b + bi) as u32) < sb.size {
            let m = (1 as i32) << (bi % 8 as i32);
            if (*bp).data[(bi / 8 as i32) as usize] as i32 & m == 0 as i32 {
                // Is block free?
                (*bp).data[(bi / 8 as i32) as usize] =
                    ((*bp).data[(bi / 8 as i32) as usize] as i32 | m) as u8; // Mark block in use.
                log_write(bp);
                brelse(bp);
                bzero(dev as i32, b + bi);
                return (b + bi) as u32;
            }
            bi += 1
        }
        brelse(bp);
        b += BPB
    }
    panic(b"balloc: out of blocks\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// Free a disk block.
unsafe fn bfree(mut dev: i32, mut b: u32) {
    let mut bp: *mut Buf = ptr::null_mut();
    let mut bi: i32 = 0;
    let mut m: i32 = 0;
    bp = bread(dev as u32, bblock(b, sb));
    bi = b.wrapping_rem(BPB as u32) as i32;
    m = (1 as i32) << (bi % 8 as i32);
    if (*bp).data[(bi / 8 as i32) as usize] as i32 & m == 0 as i32 {
        panic(b"freeing free block\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*bp).data[(bi / 8 as i32) as usize] = ((*bp).data[(bi / 8 as i32) as usize] as i32 & !m) as u8;
    log_write(bp);
    brelse(bp);
}

pub static mut icache: Icache = Icache::zeroed();

pub unsafe fn iinit() {
    icache
        .lock
        .initlock(b"icache\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    for i in 0..NINODE {
        (*icache.inode.as_mut_ptr().offset(i as isize))
            .lock
            .initlock(b"inode\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
}

/// Allocate an inode on device dev.
/// Mark it as allocated by  giving it type type.
/// Returns an unlocked but allocated and referenced inode.
pub unsafe fn ialloc(mut dev: u32, mut typ: i16) -> *mut Inode {
    for inum in 1..sb.ninodes {
        let bp = bread(dev, iblock(inum as i32, sb));
        let dip = ((*bp).data.as_mut_ptr() as *mut Dinode)
            .offset((inum as u64).wrapping_rem(IPB as u64) as isize);
        if (*dip).typ as i32 == 0 as i32 {
            // a free inode
            ptr::write_bytes(dip, 0, 1);
            (*dip).typ = typ;

            // mark it allocated on the disk
            log_write(bp);
            brelse(bp);
            return iget(dev, inum as u32);
        }
        brelse(bp);
    }
    panic(b"ialloc: no inodes\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// Find the inode with number inum on device dev
/// and return the in-memory copy. Does not lock
/// the inode and does not read it from disk.
unsafe fn iget(mut dev: u32, mut inum: u32) -> *mut Inode {
    let mut ip: *mut Inode = ptr::null_mut();
    let mut empty: *mut Inode = ptr::null_mut();

    icache.lock.acquire();

    // Is the inode already cached?
    empty = ptr::null_mut();
    ip = &mut *icache.inode.as_mut_ptr().offset(0 as i32 as isize) as *mut Inode;
    while ip < &mut *icache.inode.as_mut_ptr().offset(NINODE as isize) as *mut Inode {
        if (*ip).ref_0 > 0 as i32 && (*ip).dev == dev && (*ip).inum == inum {
            (*ip).ref_0 += 1;
            icache.lock.release();
            return ip;
        }
        if empty.is_null() && (*ip).ref_0 == 0 as i32 {
            // Remember empty slot.
            empty = ip
        }
        ip = ip.offset(1)
    }

    // Recycle an inode cache entry.
    if empty.is_null() {
        panic(b"iget: no inodes\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ip = empty;
    (*ip).dev = dev;
    (*ip).inum = inum;
    (*ip).ref_0 = 1 as i32;
    (*ip).valid = 0 as i32;
    icache.lock.release();
    ip
}

/// Copy stat information from inode.
/// Caller must hold ip->lock.
pub unsafe fn stati(mut ip: *mut Inode, mut st: *mut Stat) {
    (*st).dev = (*ip).dev as i32;
    (*st).ino = (*ip).inum;
    (*st).typ = (*ip).typ;
    (*st).nlink = (*ip).nlink;
    (*st).size = (*ip).size as u64;
}

/// Directories
pub unsafe fn namecmp(mut s: *const libc::c_char, mut t: *const libc::c_char) -> i32 {
    strncmp(s, t, DIRSIZ as u32)
}

/// Paths
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
unsafe fn skipelem(mut path: *mut libc::c_char, mut name: *mut libc::c_char) -> *mut libc::c_char {
    let mut s: *mut libc::c_char = ptr::null_mut();
    let mut len: i32 = 0;
    while *path as i32 == '/' as i32 {
        path = path.offset(1)
    }
    if *path as i32 == 0 as i32 {
        return ptr::null_mut();
    }
    s = path;
    while *path as i32 != '/' as i32 && *path as i32 != 0 as i32 {
        path = path.offset(1)
    }
    len = path.wrapping_offset_from(s) as i64 as i32;
    if len >= DIRSIZ as i32 {
        ptr::copy(s as *const libc::c_void, name as *mut libc::c_void, DIRSIZ);
    } else {
        ptr::copy(
            s as *const libc::c_void,
            name as *mut libc::c_void,
            len as usize,
        );
        *name.offset(len as isize) = 0 as i32 as libc::c_char
    }
    while *path as i32 == '/' as i32 {
        path = path.offset(1)
    }
    path
}

/// Look up and return the inode for a path name.
/// If parent != 0, return the inode for the parent and copy the final
/// path element into name, which must have room for DIRSIZ bytes.
/// Must be called inside a transaction since it calls put().
unsafe fn namex(
    mut path: *mut libc::c_char,
    mut nameiparent_0: i32,
    mut name: *mut libc::c_char,
) -> *mut Inode {
    let mut ip: *mut Inode = ptr::null_mut();
    let mut next: *mut Inode = ptr::null_mut();
    if *path as i32 == '/' as i32 {
        ip = iget(ROOTDEV as u32, ROOTINO as u32)
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
        next = (*ip).dirlookup(name, ptr::null_mut());
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

pub unsafe fn namei(mut path: *mut libc::c_char) -> *mut Inode {
    let mut name: [libc::c_char; DIRSIZ] = [0; DIRSIZ];
    namex(path, 0 as i32, name.as_mut_ptr())
}

pub unsafe fn nameiparent(mut path: *mut libc::c_char, mut name: *mut libc::c_char) -> *mut Inode {
    namex(path, 1 as i32, name)
}
