use super::{
    balloc, bfree, fs, Dirent, FileName, BSIZE, DIRENT_SIZE, IPB, MAXFILE, NDIRECT, NINDIRECT,
};

use crate::{
    arena::{Arena, Rc, RcArena, Tag},
    bio::Buf,
    kernel::kernel,
    param::NINODE,
    proc::{either_copyin, either_copyout},
    sleeplock::Sleeplock,
    spinlock::SpinlockGuard,
    stat::{Stat, T_DIR, T_NONE},
};
use core::{
    mem,
    ops::{Deref, DerefMut},
    ptr,
};

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

impl Tag for IcacheTag {
    type Target = RcArena<Inode, NINODE>;
    type Result = SpinlockGuard<'static, Self::Target>;

    fn arena(&self) -> Self::Result {
        kernel().icache.lock()
    }
}

pub type RcInode = Rc<IcacheTag>;

impl RcInode {
    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub fn lock(self) -> RcInodeGuard {
        let mut guard = self.inner.lock();
        if !guard.valid {
            let mut bp = Buf::new(self.dev, fs().superblock.iblock(self.inum));
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
        RcInodeGuard { inner: self }
    }
}

/// RcInodeGuard implies that SleeplockWIP<Inode> is held by current thread.
///
/// # Invariant
///
/// When SleeplockWIP<InodeInner> is held, InodeInner's valid is always true.
pub struct RcInodeGuard {
    pub inner: RcInode,
}

impl Deref for RcInodeGuard {
    type Target = InodeGuard;

    fn deref(&self) -> &Self::Target {
        #[allow(clippy::transmute_ptr_to_ref)]
        unsafe {
            mem::transmute(self.inner.deref() as *const _)
        }
    }
}

impl DerefMut for RcInodeGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[allow(clippy::transmute_ptr_to_ref)]
        unsafe {
            mem::transmute(self.inner.deref() as *const _)
        }
    }
}

/// Unlock and put the given inode.
impl Drop for RcInodeGuard {
    fn drop(&mut self) {
        unsafe {
            self.inner.inner.unlock();
        }
    }
}

/// RcInodeGuard implies that SleeplockWIP<Inode> is held by current thread.
///
/// # Invariant
///
/// When SleeplockWIP<InodeInner> is held, InodeInner's valid is always true.
pub struct InodeGuard {
    pub inner: &'static Inode,
}

impl Deref for InodeGuard {
    type Target = InodeInner;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.inner.get_mut_unchecked() }
    }
}

impl DerefMut for InodeGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.inner.get_mut_unchecked() }
    }
}

/// Unlock and put the given inode.
impl Drop for InodeGuard {
    fn drop(&mut self) {
        unsafe {
            self.inner.inner.unlock();
        }
    }
}

impl RcInodeGuard {
    // Directories
    /// Write a new directory entry (name, inum) into the directory dp.
    pub unsafe fn dirlink(&mut self, name: &FileName, inum: u32) -> Result<(), ()> {
        let mut de: Dirent = Default::default();

        // Check that name is not present.
        if let Ok((_ip, _)) = self.dirlookup(name) {
            return Err(());
        };

        // Look for an empty Dirent.
        let mut off: u32 = 0;
        while off < self.size {
            de.read_entry(self, off, "dirlink read");
            if de.inum == 0 {
                break;
            }
            off = (off as usize).wrapping_add(DIRENT_SIZE) as u32
        }
        de.inum = inum as u16;
        de.set_name(name);
        let bytes_write = self.write(
            false,
            &mut de as *mut Dirent as usize,
            off,
            DIRENT_SIZE as u32,
        );
        assert_eq!(bytes_write, Ok(DIRENT_SIZE), "dirlink");
        Ok(())
    }

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub unsafe fn dirlookup(&mut self, name: &FileName) -> Result<(RcInode, u32), ()> {
        let mut de: Dirent = Default::default();
        assert_eq!(self.typ, T_DIR, "dirlookup not DIR");
        for off in (0..self.size).step_by(DIRENT_SIZE) {
            de.read_entry(self, off, "dirlookup read");
            if de.inum != 0 && name == de.get_name() {
                // entry matches path element
                return Ok((Inode::get(self.inner.dev, de.inum as u32), off));
            }
        }
        Err(())
    }

    pub fn unlock(self) -> RcInode {
        unsafe {
            self.inner.inner.unlock();
        }
        // TODO(rv6): unsafe. use MauallyDrop
        let inner = unsafe { ptr::read(&self.inner) };
        mem::forget(self);
        inner
    }
}

impl InodeGuard {
    /// Copy stat information from inode.
    /// Caller must hold ip->lock.
    pub unsafe fn stat(&self) -> Stat {
        Stat {
            dev: self.inner.dev as i32,
            ino: self.inner.inum,
            typ: self.typ,
            nlink: self.nlink,
            size: self.size as usize,
        }
    }

    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    /// Caller must hold self->lock.
    pub unsafe fn update(&self) {
        let mut bp = Buf::new(self.inner.dev, fs().superblock.iblock(self.inner.inum));
        let mut dip: *mut Dinode = (bp.deref_mut_inner().data.as_mut_ptr() as *mut Dinode)
            .add((self.inner.inum as usize).wrapping_rem(IPB));
        (*dip).typ = self.typ;
        (*dip).major = self.major;
        (*dip).minor = self.minor;
        (*dip).nlink = self.nlink;
        (*dip).size = self.size;
        (*dip).addrs.copy_from_slice(&self.addrs);
        fs().log_write(bp);
    }

    /// Truncate inode (discard contents).
    /// Only called when the inode has no links
    /// to it (no directory entries referring to it)
    /// and has no in-memory reference to it (is
    /// not an open file or current directory).
    unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if self.addrs[i] != 0 {
                bfree(self.inner.dev as i32, self.addrs[i]);
                self.addrs[i] = 0
            }
        }
        if self.addrs[NDIRECT] != 0 {
            let mut bp = Buf::new(self.inner.dev, self.addrs[NDIRECT]);
            let a = bp.deref_mut_inner().data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.add(j) != 0 {
                    bfree(self.inner.dev as i32, *a.add(j));
                }
            }
            drop(bp);
            bfree(self.inner.dev as i32, self.addrs[NDIRECT]);
            self.addrs[NDIRECT] = 0
        }
        self.size = 0;
        self.update();
    }

    /// Read data from inode.
    /// Caller must hold self->lock.
    /// If user_dst==1, then dst is a user virtual address;
    /// otherwise, dst is a kernel address.
    pub unsafe fn read(
        &mut self,
        user_dst: bool,
        mut dst: usize,
        mut off: u32,
        mut n: u32,
    ) -> Result<usize, ()> {
        if off > self.size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) > self.size {
            n = self.size.wrapping_sub(off)
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = Buf::new(
                self.inner.dev,
                self.bmap((off as usize).wrapping_div(BSIZE)),
            );
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            let begin = off.wrapping_rem(BSIZE as u32) as usize;
            let end = begin + m as usize;
            if either_copyout(user_dst, dst, &bp.deref_mut_inner().data[begin..end]).is_err() {
                break;
            } else {
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
        user_src: bool,
        mut src: usize,
        mut off: u32,
        n: u32,
    ) -> Result<usize, ()> {
        if off > self.size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) as usize > MAXFILE.wrapping_mul(BSIZE) {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = Buf::new(
                self.inner.dev,
                self.bmap((off as usize).wrapping_div(BSIZE)),
            );
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyin(
                bp.deref_mut_inner()
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize),
                user_src,
                src,
                m as _,
            )
            .is_err()
            {
                break;
            } else {
                fs().log_write(bp);
                tot = tot.wrapping_add(m);
                off = off.wrapping_add(m);
                src = src.wrapping_add(m as usize)
            }
        }
        if n > 0 {
            if off > self.size {
                self.size = off
            }
            // write the i-node back to disk even if the size didn't change
            // because the loop above might have called bmap() and added a new
            // block to self->addrs[].
            self.update();
        }
        Ok(n as usize)
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
            addr = self.addrs[bn];
            if addr == 0 {
                addr = balloc(self.inner.dev);
                self.addrs[bn] = addr
            }
            return addr;
        }
        bn = (bn).wrapping_sub(NDIRECT);

        assert!(bn < NINDIRECT, "bmap: out of range");
        // Load indirect block, allocating if necessary.
        addr = self.addrs[NDIRECT];
        if addr == 0 {
            addr = balloc(self.inner.dev);
            self.addrs[NDIRECT] = addr
        }
        let mut bp = Buf::new(self.inner.dev, addr);
        let a: *mut u32 = bp.deref_mut_inner().data.as_mut_ptr() as *mut u32;
        addr = *a.add(bn);
        if addr == 0 {
            addr = balloc(self.inner.dev);
            *a.add(bn) = addr;
            fs().log_write(bp);
        }
        addr
    }

    /// Is the directory dp empty except for "." and ".." ?
    pub unsafe fn isdirempty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.size).step_by(DIRENT_SIZE) {
            let bytes_read = self.read(
                false,
                &mut de as *mut Dirent as usize,
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

impl Drop for Inode {
    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    #[allow(clippy::cast_ref_to_mut)]
    fn drop(&mut self) {
        // TODO(rv6): must be inside a transaction in case it has to free the inode.
        // let mut inode = kernel().icache.lock();

        if self.inner.get_mut().valid && self.inner.get_mut().nlink == 0 {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = self.lock();

            // TODO(rv6): we should temporarily unlocking the icache lock, but we didn't.
            // drop(inode);

            unsafe {
                ip.itrunc();
                ip.typ = 0;
                ip.update();
                ip.valid = false;
            }

            drop(ip);

            // TODO(rv6): see above.
            // inode = kernel().icache.lock();
        }
    }
}

impl Inode {
    pub fn lock(&self) -> InodeGuard {
        mem::forget(self.inner.lock());
        InodeGuard {
            inner: unsafe { &*(self as *const _) },
        }
    }

    /// Find the inode with number inum on device dev
    /// and return the in-memory copy. Does not lock
    /// the inode and does not read it from disk.
    pub unsafe fn get(dev: u32, inum: u32) -> RcInode {
        let ip = IcacheTag {}.find_or_alloc(
            |inode| inode.dev == dev && inode.inum == inum,
            |inode| {
                let inode = &mut *inode;
                inode.dev = dev;
                inode.inum = inum;
                inode.inner.get_mut().valid = false;
            },
        );
        ip.expect("iget: no inodes")
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by  giving it type type.
    /// Returns an unlocked but allocated and referenced inode.
    pub unsafe fn alloc(dev: u32, typ: i16) -> RcInode {
        for inum in 1..fs().superblock.ninodes {
            let mut bp = Buf::new(dev, fs().superblock.iblock(inum));
            let dip = (bp.deref_mut_inner().data.as_mut_ptr() as *mut Dinode)
                .add((inum as usize).wrapping_rem(IPB));

            // a free inode
            if (*dip).typ == 0 {
                ptr::write_bytes(dip, 0, 1);
                (*dip).typ = typ;

                // mark it allocated on the disk
                fs().log_write(bp);
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
