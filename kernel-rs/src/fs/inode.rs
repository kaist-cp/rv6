use super::{
    balloc, bfree, Dirent, FileName, BSIZE, DIRENT_SIZE, IPB, MAXFILE, NDIRECT, NINDIRECT,
};

use crate::{
    arena::{Arena, ArenaObject, ArrayArena, Rc},
    kernel::kernel,
    param::NINODE,
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::{Stat, T_DIR, T_NONE},
    virtio_disk::Disk,
    vm::{KVAddr, VAddr},
};
use core::{mem, ops::Deref, ptr};

pub struct InodeInner {
    /// inode has been read from disk?
    pub valid: bool,
    /// copy of disk inode
    pub typ: i16,
    pub major: u16,
    pub minor: u16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; 13],
}

/// in-memory copy of an inode
pub struct Inode {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    pub inner: Sleeplock<InodeInner>,
}

#[derive(Clone)]
pub struct IcacheTag {}

impl Deref for IcacheTag {
    type Target = Spinlock<ArrayArena<Inode, NINODE>>;

    fn deref(&self) -> &Self::Target {
        &kernel().icache
    }
}

pub type RcInode = Rc<<IcacheTag as Deref>::Target, IcacheTag>;

/// InodeGuard implies that SleeplockWIP<InodeInner> is held by current thread.
///
/// # Invariant
///
/// When SleeplockWIP<InodeInner> is held, InodeInner's valid is always true.
pub struct InodeGuard<'a> {
    pub inode: &'a Inode,
}

impl Deref for InodeGuard<'_> {
    type Target = Inode;

    fn deref(&self) -> &Self::Target {
        self.inode
    }
}

impl InodeGuard<'_> {
    pub fn deref_inner(&self) -> &InodeInner {
        unsafe { self.inner.get_mut_unchecked() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut InodeInner {
        unsafe { self.inner.get_mut_unchecked() }
    }
}

/// Unlock and put the given inode.
impl Drop for InodeGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            self.inner.unlock();
        }
    }
}

// Directories
impl InodeGuard<'_> {
    /// Write a new directory entry (name, inum) into the directory dp.
    pub fn dirlink(&mut self, name: &FileName, inum: u32) -> Result<(), ()> {
        let mut de: Dirent = Default::default();

        // Check that name is not present.
        if let Ok((_ip, _)) = self.dirlookup(name) {
            return Err(());
        };

        // Look for an empty Dirent.
        let mut off: u32 = 0;
        while off < self.deref_inner().size {
            de.read_entry(self, off, "dirlink read");
            if de.inum == 0 {
                break;
            }
            off = (off as usize).wrapping_add(DIRENT_SIZE) as u32
        }
        de.inum = inum as u16;
        de.set_name(name);
        let bytes_write = self.write(
            KVAddr::new(&mut de as *mut Dirent as usize),
            off,
            DIRENT_SIZE as u32,
        );
        assert_eq!(bytes_write, Ok(DIRENT_SIZE), "dirlink");
        Ok(())
    }

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub fn dirlookup(&mut self, name: &FileName) -> Result<(RcInode, u32), ()> {
        let mut de: Dirent = Default::default();

        assert_eq!(self.deref_inner().typ, T_DIR, "dirlookup not DIR");

        for off in (0..self.deref_inner().size).step_by(DIRENT_SIZE) {
            de.read_entry(self, off, "dirlookup read");
            if de.inum != 0 && name == de.get_name() {
                // entry matches path element
                return Ok((Inode::get(self.dev, de.inum as u32), off));
            }
        }
        Err(())
    }
}

impl InodeGuard<'_> {
    /// Copy stat information from inode.
    pub fn stat(&self) -> Stat {
        Stat {
            dev: self.dev as i32,
            ino: self.inum,
            typ: self.deref_inner().typ,
            nlink: self.deref_inner().nlink,
            size: self.deref_inner().size as usize,
        }
    }

    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    pub unsafe fn update(&self) {
        let mut bp = Disk::read(self.dev, kernel().fs().superblock.iblock(self.inum));
        let mut dip: *mut Dinode = (bp.deref_mut_inner().data.as_mut_ptr() as *mut Dinode)
            .add((self.inum as usize).wrapping_rem(IPB));
        let inner = self.deref_inner();
        (*dip).typ = inner.typ;
        (*dip).major = inner.major;
        (*dip).minor = inner.minor;
        (*dip).nlink = inner.nlink;
        (*dip).size = inner.size;
        (*dip).addrs.copy_from_slice(&inner.addrs);
        kernel().fs().log_write(bp);
    }

    /// Truncate inode (discard contents).
    /// This function is called with Inode's lock is held.
    pub unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if self.deref_inner().addrs[i] != 0 {
                bfree(self.dev as i32, self.deref_inner().addrs[i]);
                self.deref_inner_mut().addrs[i] = 0;
            }
        }
        if self.deref_inner().addrs[NDIRECT] != 0 {
            let mut bp = Disk::read(self.dev, self.deref_inner().addrs[NDIRECT]);
            let a = bp.deref_mut_inner().data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.add(j) != 0 {
                    bfree(self.dev as i32, *a.add(j));
                }
            }
            drop(bp);
            bfree(self.dev as i32, self.deref_inner().addrs[NDIRECT]);
            self.deref_inner_mut().addrs[NDIRECT] = 0
        }
        self.deref_inner_mut().size = 0;
        self.update();
    }

    /// Read data from inode.
    pub unsafe fn read<A: VAddr>(
        &mut self,
        mut dst: A,
        mut off: u32,
        mut n: u32,
    ) -> Result<usize, ()> {
        let inner = self.deref_inner();
        if off > inner.size || off.wrapping_add(n) < off {
            return Ok(0);
        }
        if off.wrapping_add(n) > inner.size {
            n = inner.size.wrapping_sub(off)
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = Disk::read(self.dev, self.bmap((off as usize).wrapping_div(BSIZE)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            let begin = off.wrapping_rem(BSIZE as u32) as usize;
            let end = begin + m as usize;
            if VAddr::copyout(dst, &bp.deref_mut_inner().data[begin..end]).is_err() {
                break;
            } else {
                tot = tot.wrapping_add(m);
                off = off.wrapping_add(m);
                dst = dst + (m as usize);
            }
        }
        Ok(tot as usize)
    }

    /// Write data to inode.
    pub fn write<A: VAddr>(&mut self, mut src: A, mut off: u32, n: u32) -> Result<usize, ()> {
        if off > self.deref_inner().size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) as usize > MAXFILE.wrapping_mul(BSIZE) {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = Disk::read(self.dev, self.bmap((off as usize).wrapping_div(BSIZE)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            let begin = off.wrapping_rem(BSIZE as u32) as usize;
            let end = begin + m as usize;
            if unsafe { VAddr::copyin(&mut bp.deref_mut_inner().data[begin..end], src) }.is_err() {
                break;
            } else {
                unsafe {
                    kernel().fs().log_write(bp);
                }
                tot = tot.wrapping_add(m);
                off = off.wrapping_add(m);
                src = src.add(m as usize);
            }
        }

        // TODO(@kimjungwow) : To pass copyin() usertest,
        // I updated Inode::write() earlier as
        // https://github.com/mit-pdos/xv6-riscv/commit/5e392531c07966fd8a6bee50e3e357c553fb2a2f
        if off > self.deref_inner().size {
            self.deref_inner_mut().size = off;
        }

        // write the i-node back to disk even if the size didn't change
        // because the loop above might have called bmap() and added a new
        // block to self->addrs[].
        unsafe {
            self.update();
        }
        Ok(tot as usize)
    }

    /// Inode content
    ///
    /// The content (data) associated with each inode is stored
    /// in blocks on the disk. The first NDIRECT block numbers
    /// are listed in self->addrs[].  The next NINDIRECT blocks are
    /// listed in block self->addrs[NDIRECT].
    /// Return the disk block address of the nth block in inode self.
    /// If there is no such block, bmap allocates one.
    fn bmap(&mut self, mut bn: usize) -> u32 {
        let mut addr: u32;
        let inner = self.deref_inner();
        if bn < NDIRECT {
            addr = inner.addrs[bn];
            if addr == 0 {
                addr = unsafe { balloc(self.dev) };
                self.deref_inner_mut().addrs[bn] = addr;
            }
            return addr;
        }
        bn = (bn).wrapping_sub(NDIRECT);

        assert!(bn < NINDIRECT, "bmap: out of range");
        // Load indirect block, allocating if necessary.
        addr = inner.addrs[NDIRECT];
        if addr == 0 {
            addr = unsafe { balloc(self.dev) };
            self.deref_inner_mut().addrs[NDIRECT] = addr;
        }
        let mut bp = Disk::read(self.dev, addr);
        let a: *mut u32 = bp.deref_mut_inner().data.as_mut_ptr() as *mut u32;
        unsafe {
            addr = *a.add(bn);
            if addr == 0 {
                addr = balloc(self.dev);
                *a.add(bn) = addr;
                kernel().fs().log_write(bp);
            }
        }
        addr
    }

    /// Is the directory dp empty except for "." and ".." ?
    pub unsafe fn isdirempty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.deref_inner().size).step_by(DIRENT_SIZE) {
            let bytes_read = self.read(
                KVAddr::new(&mut de as *mut Dirent as usize),
                off as u32,
                DIRENT_SIZE as u32,
            );
            assert_eq!(bytes_read, Ok(DIRENT_SIZE), "isdirempty: readi");
            if de.inum != 0 {
                return false;
            }
        }
        true
    }
}

/// On-disk inode structure
/// Both the kernel and user programs use this header file.
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct Dinode {
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
/// kernel().file_system.superblock.startinode. Each inode has a number, indicating its
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
/// The kernel().icache.lock spin-lock protects the allocation of icache
/// entries. Since ip->ref indicates whether an entry is free,
/// and ip->dev and ip->inum indicate which i-node an entry
/// holds, one must hold kernel().icache.lock while using any of those fields.
///
/// An ip->lock sleep-lock protects all ip-> fields other than ref,
/// dev, and inum.  One must hold ip->lock in order to
/// read or write that inode's ip->valid, ip->size, ip->type, &c.

impl ArenaObject for Inode {
    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    #[allow(clippy::cast_ref_to_mut)]
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>) {
        if self.inner.get_mut().valid && self.inner.get_mut().nlink == 0 {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = self.lock();

            A::reacquire_after(guard, move || unsafe {
                ip.itrunc();
                ip.deref_inner_mut().typ = 0;
                ip.update();
                ip.deref_inner_mut().valid = false;
                drop(ip);
            });
        }
    }
}

impl Inode {
    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub fn lock(&self) -> InodeGuard<'_> {
        let mut guard = self.inner.lock();
        if !guard.valid {
            let mut bp = Disk::read(self.dev, kernel().fs().superblock.iblock(self.inum));
            let dip: &mut Dinode = unsafe {
                &mut *((bp.deref_mut_inner().data.as_mut_ptr() as *mut Dinode)
                    .add((self.inum as usize).wrapping_rem(IPB)))
            };
            guard.typ = (*dip).typ;
            guard.major = (*dip).major as u16;
            guard.minor = (*dip).minor as u16;
            guard.nlink = (*dip).nlink;
            guard.size = (*dip).size;
            guard.addrs.copy_from_slice(&(*dip).addrs);
            drop(bp);
            guard.valid = true;
            assert_ne!(guard.typ, T_NONE, "Inode::lock: no type");
        };
        mem::forget(guard);
        InodeGuard { inode: self }
    }

    /// Find the inode with number inum on device dev
    /// and return the in-memory copy. Does not lock
    /// the inode and does not read it from disk.
    pub fn get(dev: u32, inum: u32) -> RcInode {
        IcacheTag {}
            .find_or_alloc(
                |inode| inode.dev == dev && inode.inum == inum,
                |inode| {
                    inode.dev = dev;
                    inode.inum = inum;
                    inode.inner.get_mut().valid = false;
                },
            )
            .expect("iget: no inodes")
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by  giving it type type.
    /// Returns an unlocked but allocated and referenced inode.
    pub unsafe fn alloc(dev: u32, typ: i16) -> RcInode {
        for inum in 1..kernel().fs().superblock.ninodes {
            let mut bp = Disk::read(dev, kernel().fs().superblock.iblock(inum));
            let dip = (bp.deref_mut_inner().data.as_mut_ptr() as *mut Dinode)
                .add((inum as usize).wrapping_rem(IPB));

            // a free inode
            if (*dip).typ == 0 {
                ptr::write_bytes(dip, 0, 1);
                (*dip).typ = typ;

                // mark it allocated on the disk
                kernel().fs().log_write(bp);
                return Inode::get(dev, inum);
            }
        }
        panic!("Inode::alloc: no inodes");
    }

    pub const fn zero() -> Self {
        Self {
            dev: 0,
            inum: 0,
            inner: Sleeplock::new(
                "inode",
                InodeInner {
                    valid: false,
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
