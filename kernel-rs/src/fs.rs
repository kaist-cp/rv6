use crate::bio::{bread, brelse};
use crate::log::{initlog, log_write};
use crate::printf::panic;
use crate::string::{memmove, memset, strncmp, strncpy};
use crate::{buf, file, libc, proc, sleeplock, spinlock, stat};
use buf::Buf;
use core::ptr;
use file::inode;
use proc::{cpu, either_copyin, either_copyout, myproc};
use sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep, Sleeplock};
use spinlock::{acquire, initlock, release, Spinlock};
use stat::Stat;
pub type uint = libc::c_uint;
pub type ushort = libc::c_ushort;
pub type uchar = libc::c_uchar;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;

pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;

/// block size
/// Disk layout:
/// [ boot block | super block | log | inode blocks |
///                                          free bit map | data blocks]
///
/// mkfs computes the super block and builds an initial file system. The
/// super block describes the disk layout:
#[derive(Copy, Clone)]
#[repr(C)]
pub struct superblock {
    pub magic: uint,
    pub size: uint,
    pub nblocks: uint,
    pub ninodes: uint,
    pub nlog: uint,
    pub logstart: uint,
    pub inodestart: uint,
    pub bmapstart: uint,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct dirent {
    pub inum: ushort,
    pub name: [libc::c_char; 14],
}
/// On-disk inode structure
#[derive(Copy, Clone)]
#[repr(C)]
pub struct dinode {
    pub type_0: libc::c_short,
    pub major: libc::c_short,
    pub minor: libc::c_short,
    pub nlink: libc::c_short,
    pub size: uint,
    pub addrs: [uint; 13],
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
///   is non-zero. ialloc() allocates, and iput() frees if
///   the reference and link counts have fallen to zero.
///
/// * Referencing in cache: an entry in the inode cache
///   is free if ip->ref is zero. Otherwise ip->ref tracks
///   the number of in-memory pointers to the entry (open
///   files and current directories). iget() finds or
///   creates a cache entry and increments its ref; iput()
///   decrements ref.
///
/// * Valid: the information (type, size, &c) in an inode
///   cache entry is only correct when ip->valid is 1.
///   ilock() reads the inode from
///   the disk and sets ip->valid, while iput() clears
///   ip->valid if ip->ref has fallen to zero.
///
/// * Locked: file system code may only examine and modify
///   the information in an inode and its content if it
///   has first locked the inode.
///
/// Thus a typical sequence is:
///   ip = iget(dev, inum)
///   ilock(ip)
///   ... examine and modify ip->xxx ...
///   iunlock(ip)
///   iput(ip)
///
/// ilock() is separate from iget() so that system calls can
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
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub lock: Spinlock,
    pub inode: [inode; 50],
}
// maximum number of processes
// maximum number of CPUs
// open files per process
// open files per system
pub const NINODE: libc::c_int = 50 as libc::c_int;
// maximum number of active i-nodes
// maximum major device number
pub const ROOTDEV: libc::c_int = 1 as libc::c_int;
pub const T_DIR: libc::c_int = 1 as libc::c_int;
// On-disk file system format.
// Both the kernel and user programs use this header file.
pub const ROOTINO: libc::c_int = 1 as libc::c_int;
// root i-number
pub const BSIZE: libc::c_int = 1024 as libc::c_int;
// Block number of first free map block
pub const FSMAGIC: libc::c_int = 0x10203040 as libc::c_int;
pub const NDIRECT: libc::c_int = 12 as libc::c_int;
// Block containing inode i
// Bitmap bits per block
pub const BPB: libc::c_int = BSIZE * 8 as libc::c_int;
// Block of free map containing bit for block b
// Directory is a file containing a sequence of dirent structures.
pub const DIRSIZ: libc::c_int = 14 as libc::c_int;
// there should be one superblock per disk device, but we run with
// only one device
#[no_mangle]
pub static mut sb: superblock = superblock {
    magic: 0,
    size: 0,
    nblocks: 0,
    ninodes: 0,
    nlog: 0,
    logstart: 0,
    inodestart: 0,
    bmapstart: 0,
};
/// Read the super block.
unsafe extern "C" fn readsb(mut dev: libc::c_int, mut sb_0: *mut superblock) {
    let mut bp: *mut Buf = ptr::null_mut();
    bp = bread(dev as uint, 1 as libc::c_int as uint);
    memmove(
        sb_0 as *mut libc::c_void,
        (*bp).data.as_mut_ptr() as *const libc::c_void,
        ::core::mem::size_of::<superblock>() as libc::c_ulong as uint,
    );
    brelse(bp);
}
/// Init fs
#[no_mangle]
pub unsafe extern "C" fn fsinit(mut dev: libc::c_int) {
    readsb(dev, &mut sb);
    if sb.magic != FSMAGIC as libc::c_uint {
        panic(b"invalid file system\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    initlog(dev, &mut sb);
}
/// Zero a block.
unsafe extern "C" fn bzero(mut dev: libc::c_int, mut bno: libc::c_int) {
    let mut bp: *mut Buf = ptr::null_mut();
    bp = bread(dev as uint, bno as uint);
    memset(
        (*bp).data.as_mut_ptr() as *mut libc::c_void,
        0 as libc::c_int,
        BSIZE as uint,
    );
    log_write(bp);
    brelse(bp);
}
/// Blocks.
/// Allocate a zeroed disk block.
unsafe extern "C" fn balloc(mut dev: uint) -> uint {
    let mut b: libc::c_int = 0;
    let mut bi: libc::c_int = 0;
    let mut m: libc::c_int = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    bp = ptr::null_mut();
    b = 0 as libc::c_int;
    while (b as libc::c_uint) < sb.size {
        bp = bread(dev, ((b / BPB) as libc::c_uint).wrapping_add(sb.bmapstart));
        bi = 0 as libc::c_int;
        while bi < BPB && ((b + bi) as libc::c_uint) < sb.size {
            m = (1 as libc::c_int) << (bi % 8 as libc::c_int);
            if (*bp).data[(bi / 8 as libc::c_int) as usize] as libc::c_int & m == 0 as libc::c_int {
                // Is block free?
                (*bp).data[(bi / 8 as libc::c_int) as usize] =
                    ((*bp).data[(bi / 8 as libc::c_int) as usize] as libc::c_int | m) as uchar; // Mark block in use.
                log_write(bp);
                brelse(bp);
                bzero(dev as libc::c_int, b + bi);
                return (b + bi) as uint;
            }
            bi += 1
        }
        brelse(bp);
        b += BPB
    }
    panic(b"balloc: out of blocks\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
/// Free a disk block.
unsafe extern "C" fn bfree(mut dev: libc::c_int, mut b: uint) {
    let mut bp: *mut Buf = ptr::null_mut();
    let mut bi: libc::c_int = 0;
    let mut m: libc::c_int = 0;
    bp = bread(
        dev as uint,
        b.wrapping_div(BPB as libc::c_uint)
            .wrapping_add(sb.bmapstart),
    );
    bi = b.wrapping_rem(BPB as libc::c_uint) as libc::c_int;
    m = (1 as libc::c_int) << (bi % 8 as libc::c_int);
    if (*bp).data[(bi / 8 as libc::c_int) as usize] as libc::c_int & m == 0 as libc::c_int {
        panic(b"freeing free block\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*bp).data[(bi / 8 as libc::c_int) as usize] =
        ((*bp).data[(bi / 8 as libc::c_int) as usize] as libc::c_int & !m) as uchar;
    log_write(bp);
    brelse(bp);
}
#[no_mangle]
pub static mut icache: C2RustUnnamed_0 = C2RustUnnamed_0 {
    lock: Spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    inode: [inode {
        dev: 0,
        inum: 0,
        ref_0: 0,
        lock: Sleeplock {
            locked: 0,
            lk: Spinlock {
                locked: 0,
                name: 0 as *const libc::c_char as *mut libc::c_char,
                cpu: 0 as *const cpu as *mut cpu,
            },
            name: 0 as *const libc::c_char as *mut libc::c_char,
            pid: 0,
        },
        valid: 0,
        type_0: 0,
        major: 0,
        minor: 0,
        nlink: 0,
        size: 0,
        addrs: [0; 13],
    }; 50],
};
#[no_mangle]
pub unsafe extern "C" fn iinit() {
    let mut i: libc::c_int = 0 as libc::c_int;
    initlock(
        &mut icache.lock,
        b"icache\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    i = 0 as libc::c_int;
    while i < NINODE {
        initsleeplock(
            &mut (*icache.inode.as_mut_ptr().offset(i as isize)).lock,
            b"inode\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
        i += 1
    }
}
/// Allocate an inode on device dev.
/// Mark it as allocated by  giving it type type.
/// Returns an unlocked but allocated and referenced inode.
#[no_mangle]
pub unsafe extern "C" fn ialloc(mut dev: uint, mut type_0: libc::c_short) -> *mut inode {
    let mut inum: libc::c_int = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    let mut dip: *mut dinode = ptr::null_mut();
    inum = 1 as libc::c_int;
    while (inum as libc::c_uint) < sb.ninodes {
        bp = bread(
            dev,
            (inum as libc::c_ulong)
                .wrapping_div(
                    (BSIZE as libc::c_ulong)
                        .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
                )
                .wrapping_add(sb.inodestart as libc::c_ulong) as uint,
        );
        dip = ((*bp).data.as_mut_ptr() as *mut dinode).offset(
            (inum as libc::c_ulong).wrapping_rem(
                (BSIZE as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
            ) as isize,
        );
        if (*dip).type_0 as libc::c_int == 0 as libc::c_int {
            // a free inode
            memset(
                dip as *mut libc::c_void,
                0 as libc::c_int,
                ::core::mem::size_of::<dinode>() as libc::c_ulong as uint,
            ); // mark it allocated on the disk
            (*dip).type_0 = type_0;
            log_write(bp);
            brelse(bp);
            return iget(dev, inum as uint);
        }
        brelse(bp);
        inum += 1
    }
    panic(b"ialloc: no inodes\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
/// Copy a modified in-memory inode to disk.
/// Must be called after every change to an ip->xxx field
/// that lives on disk, since i-node cache is write-through.
/// Caller must hold ip->lock.
#[no_mangle]
pub unsafe extern "C" fn iupdate(mut ip: *mut inode) {
    let mut bp: *mut Buf = ptr::null_mut();
    let mut dip: *mut dinode = ptr::null_mut();
    bp = bread(
        (*ip).dev,
        ((*ip).inum as libc::c_ulong)
            .wrapping_div(
                (BSIZE as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
            )
            .wrapping_add(sb.inodestart as libc::c_ulong) as uint,
    );
    dip = ((*bp).data.as_mut_ptr() as *mut dinode).offset(
        ((*ip).inum as libc::c_ulong).wrapping_rem(
            (BSIZE as libc::c_ulong)
                .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
        ) as isize,
    );
    (*dip).type_0 = (*ip).type_0;
    (*dip).major = (*ip).major;
    (*dip).minor = (*ip).minor;
    (*dip).nlink = (*ip).nlink;
    (*dip).size = (*ip).size;
    memmove(
        (*dip).addrs.as_mut_ptr() as *mut libc::c_void,
        (*ip).addrs.as_mut_ptr() as *const libc::c_void,
        ::core::mem::size_of::<[uint; 13]>() as libc::c_ulong as uint,
    );
    log_write(bp);
    brelse(bp);
}
/// Find the inode with number inum on device dev
/// and return the in-memory copy. Does not lock
/// the inode and does not read it from disk.
unsafe extern "C" fn iget(mut dev: uint, mut inum: uint) -> *mut inode {
    let mut ip: *mut inode = ptr::null_mut();
    let mut empty: *mut inode = ptr::null_mut();
    acquire(&mut icache.lock);
    // Is the inode already cached?
    empty = ptr::null_mut();
    ip = &mut *icache.inode.as_mut_ptr().offset(0 as libc::c_int as isize) as *mut inode;
    while ip < &mut *icache.inode.as_mut_ptr().offset(NINODE as isize) as *mut inode {
        if (*ip).ref_0 > 0 as libc::c_int && (*ip).dev == dev && (*ip).inum == inum {
            (*ip).ref_0 += 1;
            release(&mut icache.lock);
            return ip;
        }
        if empty.is_null() && (*ip).ref_0 == 0 as libc::c_int {
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
    (*ip).ref_0 = 1 as libc::c_int;
    (*ip).valid = 0 as libc::c_int;
    release(&mut icache.lock);
    ip
}
/// Increment reference count for ip.
/// Returns ip to enable ip = idup(ip1) idiom.
#[no_mangle]
pub unsafe extern "C" fn idup(mut ip: *mut inode) -> *mut inode {
    acquire(&mut icache.lock);
    (*ip).ref_0 += 1;
    release(&mut icache.lock);
    ip
}
/// Lock the given inode.
/// Reads the inode from disk if necessary.
#[no_mangle]
pub unsafe extern "C" fn ilock(mut ip: *mut inode) {
    let mut bp: *mut Buf = ptr::null_mut();
    let mut dip: *mut dinode = ptr::null_mut();
    if ip.is_null() || (*ip).ref_0 < 1 as libc::c_int {
        panic(b"ilock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    acquiresleep(&mut (*ip).lock);
    if (*ip).valid == 0 as libc::c_int {
        bp = bread(
            (*ip).dev,
            ((*ip).inum as libc::c_ulong)
                .wrapping_div(
                    (BSIZE as libc::c_ulong)
                        .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
                )
                .wrapping_add(sb.inodestart as libc::c_ulong) as uint,
        );
        dip = ((*bp).data.as_mut_ptr() as *mut dinode).offset(
            ((*ip).inum as libc::c_ulong).wrapping_rem(
                (BSIZE as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<dinode>() as libc::c_ulong),
            ) as isize,
        );
        (*ip).type_0 = (*dip).type_0;
        (*ip).major = (*dip).major;
        (*ip).minor = (*dip).minor;
        (*ip).nlink = (*dip).nlink;
        (*ip).size = (*dip).size;
        memmove(
            (*ip).addrs.as_mut_ptr() as *mut libc::c_void,
            (*dip).addrs.as_mut_ptr() as *const libc::c_void,
            ::core::mem::size_of::<[uint; 13]>() as libc::c_ulong as uint,
        );
        brelse(bp);
        (*ip).valid = 1 as libc::c_int;
        if (*ip).type_0 as libc::c_int == 0 as libc::c_int {
            panic(b"ilock: no type\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
    };
}
/// Unlock the given inode.
#[no_mangle]
pub unsafe extern "C" fn iunlock(mut ip: *mut inode) {
    if ip.is_null() || holdingsleep(&mut (*ip).lock) == 0 || (*ip).ref_0 < 1 as libc::c_int {
        panic(b"iunlock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    releasesleep(&mut (*ip).lock);
}
/// Drop a reference to an in-memory inode.
/// If that was the last reference, the inode cache entry can
/// be recycled.
/// If that was the last reference and the inode has no links
/// to it, free the inode (and its content) on disk.
/// All calls to iput() must be inside a transaction in
/// case it has to free the inode.
#[no_mangle]
pub unsafe extern "C" fn iput(mut ip: *mut inode) {
    acquire(&mut icache.lock);
    if (*ip).ref_0 == 1 as libc::c_int
        && (*ip).valid != 0
        && (*ip).nlink as libc::c_int == 0 as libc::c_int
    {
        // inode has no links and no other references: truncate and free.
        // ip->ref == 1 means no other process can have ip locked,
        // so this acquiresleep() won't block (or deadlock).
        acquiresleep(&mut (*ip).lock);
        release(&mut icache.lock);
        itrunc(ip);
        (*ip).type_0 = 0 as libc::c_int as libc::c_short;
        iupdate(ip);
        (*ip).valid = 0 as libc::c_int;
        releasesleep(&mut (*ip).lock);
        acquire(&mut icache.lock);
    }
    (*ip).ref_0 -= 1;
    release(&mut icache.lock);
}
/// Common idiom: unlock, then put.
#[no_mangle]
pub unsafe extern "C" fn iunlockput(mut ip: *mut inode) {
    iunlock(ip);
    iput(ip);
}
/// Inode content
///
/// The content (data) associated with each inode is stored
/// in blocks on the disk. The first NDIRECT block numbers
/// are listed in ip->addrs[].  The next NINDIRECT blocks are
/// listed in block ip->addrs[NDIRECT].
/// Return the disk block address of the nth block in inode ip.
/// If there is no such block, bmap allocates one.
unsafe extern "C" fn bmap(mut ip: *mut inode, mut bn: uint) -> uint {
    let mut addr: uint = 0;
    let mut a: *mut uint = ptr::null_mut();
    let mut bp: *mut Buf = ptr::null_mut();
    if bn < NDIRECT as libc::c_uint {
        addr = (*ip).addrs[bn as usize];
        if addr == 0 as libc::c_int as libc::c_uint {
            addr = balloc((*ip).dev);
            (*ip).addrs[bn as usize] = addr
        }
        return addr;
    }
    bn = (bn as libc::c_uint).wrapping_sub(NDIRECT as libc::c_uint) as uint as uint;
    if (bn as libc::c_ulong)
        < (BSIZE as libc::c_ulong).wrapping_div(::core::mem::size_of::<uint>() as libc::c_ulong)
    {
        // Load indirect block, allocating if necessary.
        addr = (*ip).addrs[NDIRECT as usize];
        if addr == 0 as libc::c_int as libc::c_uint {
            addr = balloc((*ip).dev);
            (*ip).addrs[NDIRECT as usize] = addr
        }
        bp = bread((*ip).dev, addr);
        a = (*bp).data.as_mut_ptr() as *mut uint;
        addr = *a.offset(bn as isize);
        if addr == 0 as libc::c_int as libc::c_uint {
            addr = balloc((*ip).dev);
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
unsafe extern "C" fn itrunc(mut ip: *mut inode) {
    let mut i: libc::c_int = 0;
    let mut j: libc::c_int = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    let mut a: *mut uint = ptr::null_mut();
    i = 0 as libc::c_int;
    while i < NDIRECT {
        if (*ip).addrs[i as usize] != 0 {
            bfree((*ip).dev as libc::c_int, (*ip).addrs[i as usize]);
            (*ip).addrs[i as usize] = 0 as libc::c_int as uint
        }
        i += 1
    }
    if (*ip).addrs[NDIRECT as usize] != 0 {
        bp = bread((*ip).dev, (*ip).addrs[NDIRECT as usize]);
        a = (*bp).data.as_mut_ptr() as *mut uint;
        j = 0 as libc::c_int;
        while (j as libc::c_ulong)
            < (BSIZE as libc::c_ulong).wrapping_div(::core::mem::size_of::<uint>() as libc::c_ulong)
        {
            if *a.offset(j as isize) != 0 {
                bfree((*ip).dev as libc::c_int, *a.offset(j as isize));
            }
            j += 1
        }
        brelse(bp);
        bfree((*ip).dev as libc::c_int, (*ip).addrs[NDIRECT as usize]);
        (*ip).addrs[NDIRECT as usize] = 0 as libc::c_int as uint
    }
    (*ip).size = 0 as libc::c_int as uint;
    iupdate(ip);
}
/// Copy stat information from inode.
/// Caller must hold ip->lock.
#[no_mangle]
pub unsafe extern "C" fn stati(mut ip: *mut inode, mut st: *mut Stat) {
    (*st).dev = (*ip).dev as libc::c_int;
    (*st).ino = (*ip).inum;
    (*st).type_0 = (*ip).type_0;
    (*st).nlink = (*ip).nlink;
    (*st).size = (*ip).size as uint64;
}
/// Read data from inode.
/// Caller must hold ip->lock.
/// If user_dst==1, then dst is a user virtual address;
/// otherwise, dst is a kernel address.
#[no_mangle]
pub unsafe extern "C" fn readi(
    mut ip: *mut inode,
    mut user_dst: libc::c_int,
    mut dst: uint64,
    mut off: uint,
    mut n: uint,
) -> libc::c_int {
    let mut tot: uint = 0;
    let mut m: uint = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    if off > (*ip).size || off.wrapping_add(n) < off {
        return -(1 as libc::c_int);
    }
    if off.wrapping_add(n) > (*ip).size {
        n = (*ip).size.wrapping_sub(off)
    }
    tot = 0 as libc::c_int as uint;
    while tot < n {
        bp = bread((*ip).dev, bmap(ip, off.wrapping_div(BSIZE as libc::c_uint)));
        m = if n.wrapping_sub(tot)
            < (1024 as libc::c_int as libc::c_uint)
                .wrapping_sub(off.wrapping_rem(1024 as libc::c_int as libc::c_uint))
        {
            n.wrapping_sub(tot)
        } else {
            (1024 as libc::c_int as libc::c_uint)
                .wrapping_sub(off.wrapping_rem(1024 as libc::c_int as libc::c_uint))
        };
        if either_copyout(
            user_dst,
            dst,
            (*bp)
                .data
                .as_mut_ptr()
                .offset(off.wrapping_rem(BSIZE as libc::c_uint) as isize)
                as *mut libc::c_void,
            m as uint64,
        ) == -(1 as libc::c_int)
        {
            brelse(bp);
            break;
        } else {
            brelse(bp);
            tot = (tot as libc::c_uint).wrapping_add(m) as uint as uint;
            off = (off as libc::c_uint).wrapping_add(m) as uint as uint;
            dst = (dst as libc::c_ulong).wrapping_add(m as libc::c_ulong) as uint64 as uint64
        }
    }
    n as libc::c_int
}
/// Write data to inode.
/// Caller must hold ip->lock.
/// If user_src==1, then src is a user virtual address;
/// otherwise, src is a kernel address.
#[no_mangle]
pub unsafe extern "C" fn writei(
    mut ip: *mut inode,
    mut user_src: libc::c_int,
    mut src: uint64,
    mut off: uint,
    mut n: uint,
) -> libc::c_int {
    let mut tot: uint = 0;
    let mut m: uint = 0;
    let mut bp: *mut Buf = ptr::null_mut();
    if off > (*ip).size || off.wrapping_add(n) < off {
        return -(1 as libc::c_int);
    }
    if off.wrapping_add(n) as libc::c_ulong
        > (NDIRECT as libc::c_ulong)
            .wrapping_add(
                (BSIZE as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<uint>() as libc::c_ulong),
            )
            .wrapping_mul(BSIZE as libc::c_ulong)
    {
        return -(1 as libc::c_int);
    }
    tot = 0 as libc::c_int as uint;
    while tot < n {
        bp = bread((*ip).dev, bmap(ip, off.wrapping_div(BSIZE as libc::c_uint)));
        m = if n.wrapping_sub(tot)
            < (1024 as libc::c_int as libc::c_uint)
                .wrapping_sub(off.wrapping_rem(1024 as libc::c_int as libc::c_uint))
        {
            n.wrapping_sub(tot)
        } else {
            (1024 as libc::c_int as libc::c_uint)
                .wrapping_sub(off.wrapping_rem(1024 as libc::c_int as libc::c_uint))
        };
        if either_copyin(
            (*bp)
                .data
                .as_mut_ptr()
                .offset(off.wrapping_rem(BSIZE as libc::c_uint) as isize)
                as *mut libc::c_void,
            user_src,
            src,
            m as uint64,
        ) == -(1 as libc::c_int)
        {
            brelse(bp);
            break;
        } else {
            log_write(bp);
            brelse(bp);
            tot = (tot as libc::c_uint).wrapping_add(m) as uint as uint;
            off = (off as libc::c_uint).wrapping_add(m) as uint as uint;
            src = (src as libc::c_ulong).wrapping_add(m as libc::c_ulong) as uint64 as uint64
        }
    }
    if n > 0 as libc::c_int as libc::c_uint {
        if off > (*ip).size {
            (*ip).size = off
        }
        // write the i-node back to disk even if the size didn't change
        // because the loop above might have called bmap() and added a new
        // block to ip->addrs[].
        iupdate(ip);
    }
    n as libc::c_int
}
/// Directories
#[no_mangle]
pub unsafe extern "C" fn namecmp(
    mut s: *const libc::c_char,
    mut t: *const libc::c_char,
) -> libc::c_int {
    strncmp(s, t, DIRSIZ as uint)
}
/// Look for a directory entry in a directory.
/// If found, set *poff to byte offset of entry.
#[no_mangle]
pub unsafe extern "C" fn dirlookup(
    mut dp: *mut inode,
    mut name: *mut libc::c_char,
    mut poff: *mut uint,
) -> *mut inode {
    let mut off: uint = 0;
    let mut inum: uint = 0;
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    if (*dp).type_0 as libc::c_int != T_DIR {
        panic(b"dirlookup not DIR\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    off = 0 as libc::c_int as uint;
    while off < (*dp).size {
        if readi(
            dp,
            0 as libc::c_int,
            &mut de as *mut dirent as uint64,
            off,
            ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
        ) as libc::c_ulong
            != ::core::mem::size_of::<dirent>() as libc::c_ulong
        {
            panic(b"dirlookup read\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        if de.inum as libc::c_int != 0 as libc::c_int
            && namecmp(name, de.name.as_mut_ptr()) == 0 as libc::c_int
        {
            // entry matches path element
            if !poff.is_null() {
                *poff = off
            }
            inum = de.inum as uint;
            return iget((*dp).dev, inum);
        }
        off = (off as libc::c_ulong).wrapping_add(::core::mem::size_of::<dirent>() as libc::c_ulong)
            as uint as uint
    }
    ptr::null_mut()
}
/// Write a new directory entry (name, inum) into the directory dp.
#[no_mangle]
pub unsafe extern "C" fn dirlink(
    mut dp: *mut inode,
    mut name: *mut libc::c_char,
    mut inum: uint,
) -> libc::c_int {
    let mut off: libc::c_int = 0;
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    let mut ip: *mut inode = ptr::null_mut();
    // Check that name is not present.
    ip = dirlookup(dp, name, ptr::null_mut());
    if !ip.is_null() {
        iput(ip);
        return -(1 as libc::c_int);
    }
    // Look for an empty dirent.
    off = 0 as libc::c_int;
    while (off as libc::c_uint) < (*dp).size {
        if readi(
            dp,
            0 as libc::c_int,
            &mut de as *mut dirent as uint64,
            off as uint,
            ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
        ) as libc::c_ulong
            != ::core::mem::size_of::<dirent>() as libc::c_ulong
        {
            panic(b"dirlink read\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        if de.inum as libc::c_int == 0 as libc::c_int {
            break;
        }
        off = (off as libc::c_ulong).wrapping_add(::core::mem::size_of::<dirent>() as libc::c_ulong)
            as libc::c_int as libc::c_int
    }
    strncpy(de.name.as_mut_ptr(), name, DIRSIZ);
    de.inum = inum as ushort;
    if writei(
        dp,
        0 as libc::c_int,
        &mut de as *mut dirent as uint64,
        off as uint,
        ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
    ) as libc::c_ulong
        != ::core::mem::size_of::<dirent>() as libc::c_ulong
    {
        panic(b"dirlink\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    0 as libc::c_int
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
///
unsafe extern "C" fn skipelem(
    mut path: *mut libc::c_char,
    mut name: *mut libc::c_char,
) -> *mut libc::c_char {
    let mut s: *mut libc::c_char = ptr::null_mut();
    let mut len: libc::c_int = 0;
    while *path as libc::c_int == '/' as i32 {
        path = path.offset(1)
    }
    if *path as libc::c_int == 0 as libc::c_int {
        return ptr::null_mut();
    }
    s = path;
    while *path as libc::c_int != '/' as i32 && *path as libc::c_int != 0 as libc::c_int {
        path = path.offset(1)
    }
    len = path.wrapping_offset_from(s) as libc::c_long as libc::c_int;
    if len >= DIRSIZ {
        memmove(
            name as *mut libc::c_void,
            s as *const libc::c_void,
            DIRSIZ as uint,
        );
    } else {
        memmove(
            name as *mut libc::c_void,
            s as *const libc::c_void,
            len as uint,
        );
        *name.offset(len as isize) = 0 as libc::c_int as libc::c_char
    }
    while *path as libc::c_int == '/' as i32 {
        path = path.offset(1)
    }
    path
}
/// Look up and return the inode for a path name.
/// If parent != 0, return the inode for the parent and copy the final
/// path element into name, which must have room for DIRSIZ bytes.
/// Must be called inside a transaction since it calls iput().
unsafe extern "C" fn namex(
    mut path: *mut libc::c_char,
    mut nameiparent_0: libc::c_int,
    mut name: *mut libc::c_char,
) -> *mut inode {
    let mut ip: *mut inode = ptr::null_mut();
    let mut next: *mut inode = ptr::null_mut();
    if *path as libc::c_int == '/' as i32 {
        ip = iget(ROOTDEV as uint, ROOTINO as uint)
    } else {
        ip = idup((*myproc()).cwd)
    }
    loop {
        path = skipelem(path, name);
        if path.is_null() {
            break;
        }
        ilock(ip);
        if (*ip).type_0 as libc::c_int != T_DIR {
            iunlockput(ip);
            return ptr::null_mut();
        }
        if nameiparent_0 != 0 && *path as libc::c_int == '\u{0}' as i32 {
            // Stop one level early.
            iunlock(ip);
            return ip;
        }
        next = dirlookup(ip, name, ptr::null_mut());
        if next.is_null() {
            iunlockput(ip);
            return ptr::null_mut();
        }
        iunlockput(ip);
        ip = next
    }
    if nameiparent_0 != 0 {
        iput(ip);
        return ptr::null_mut();
    }
    ip
}
#[no_mangle]
pub unsafe extern "C" fn namei(mut path: *mut libc::c_char) -> *mut inode {
    let mut name: [libc::c_char; 14] = [0; 14];
    namex(path, 0 as libc::c_int, name.as_mut_ptr())
}
#[no_mangle]
pub unsafe extern "C" fn nameiparent(
    mut path: *mut libc::c_char,
    mut name: *mut libc::c_char,
) -> *mut inode {
    namex(path, 1 as libc::c_int, name)
}
