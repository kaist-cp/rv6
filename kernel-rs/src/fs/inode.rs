use super::{
    balloc, bfree, brelease, fs, iget, Buf, Dinode, Dirent, FileName, BSIZE, DIRENT_SIZE, ICACHE,
    IPB, MAXFILE, NDIRECT, NINDIRECT,
};
use crate::{
    proc::{either_copyin, either_copyout},
    sleeplock::{SleepLockGuard, SleeplockWIP},
    stat::{Stat, T_DIR, T_NONE},
};
use core::ops::{Deref, DerefMut};
use core::ptr;
/// InodeGuard implies that SleeplockWIP<Inode> is held by current thread.
///
/// # Invariant
///
/// When SleeplockWIP<InodeInner> is held, InodeInner's valid is always true.
pub struct InodeGuard<'a> {
    guard: SleepLockGuard<'a, InodeInner>,
    pub ptr: &'a Inode,
}

impl<'a> InodeGuard<'a> {
    pub const fn new(guard: SleepLockGuard<'a, InodeInner>, ptr: &'a Inode) -> Self {
        Self { guard, ptr }
    }
}

impl Deref for InodeGuard<'_> {
    type Target = InodeInner;
    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl DerefMut for InodeGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.guard
    }
}

/// Unlock the given inode.
impl Drop for InodeGuard<'_> {
    fn drop(&mut self) {
        // TODO: Reasoning why.
        assert!(self.ptr.ref_0 >= 1, "Inode::drop");
    }
}

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

    /// Reference count
    pub ref_0: i32,

    pub inner: SleeplockWIP<InodeInner>,
}

//TODO(@kimjungwow) : move inode-related methods to another file
impl InodeGuard<'_> {
    /// Common idiom: unlock, then put.
    pub unsafe fn unlockput(self) {
        let ptr = self.ptr;
        drop(self);
        ptr.put();
    }

    /// Copy stat information from inode.
    /// Caller must hold ip->lock.
    pub unsafe fn stat(&self) -> Stat {
        Stat {
            dev: self.ptr.dev as i32,
            ino: self.ptr.inum,
            typ: self.typ,
            nlink: self.nlink,
            size: self.size as usize,
        }
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
        let bytes_write = self.write(0, &mut de as *mut Dirent as usize, off, DIRENT_SIZE as u32);
        assert_eq!(bytes_write, Ok(DIRENT_SIZE), "dirlink");
        Ok(())
    }

    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk, since i-node cache is write-through.
    /// Caller must hold self->lock.
    pub unsafe fn update(&self) {
        let bp: *mut Buf = Buf::read(self.ptr.dev, fs().superblock.iblock(self.ptr.inum));
        let mut dip: *mut Dinode = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
            .add((self.ptr.inum as usize).wrapping_rem(IPB));
        (*dip).typ = self.typ;
        (*dip).major = self.major;
        (*dip).minor = self.minor;
        (*dip).nlink = self.nlink;
        (*dip).size = self.size;
        (*dip).addrs.copy_from_slice(&self.addrs);
        fs().log_write(bp);
        brelease(&mut *bp);
    }

    /// Truncate inode (discard contents).
    /// Only called when the inode has no links
    /// to it (no directory entries referring to it)
    /// and has no in-memory reference to it (is
    /// not an open file or current directory).
    unsafe fn itrunc(&mut self) {
        for i in 0..NDIRECT {
            if self.addrs[i] != 0 {
                bfree(self.ptr.dev as i32, self.addrs[i]);
                self.addrs[i] = 0
            }
        }
        if self.addrs[NDIRECT] != 0 {
            let bp = Buf::read(self.ptr.dev, self.addrs[NDIRECT]);
            let a = (*bp).inner.data.as_mut_ptr() as *mut u32;
            for j in 0..NINDIRECT {
                if *a.add(j) != 0 {
                    bfree(self.ptr.dev as i32, *a.add(j));
                }
            }
            brelease(&mut *bp);
            bfree(self.ptr.dev as i32, self.addrs[NDIRECT]);
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
        user_dst: i32,
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
            let bp = Buf::read(self.ptr.dev, self.bmap((off as usize).wrapping_div(BSIZE)));
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
                    .offset(off.wrapping_rem(BSIZE as u32) as isize),
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
        if off > self.size || off.wrapping_add(n) < off {
            return Err(());
        }
        if off.wrapping_add(n) as usize > MAXFILE.wrapping_mul(BSIZE) {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = Buf::read(self.ptr.dev, self.bmap((off as usize).wrapping_div(BSIZE)));
            let m = core::cmp::min(
                n.wrapping_sub(tot),
                (BSIZE as u32).wrapping_sub(off.wrapping_rem(BSIZE as u32)),
            );
            if either_copyin(
                (*bp)
                    .inner
                    .data
                    .as_mut_ptr()
                    .offset(off.wrapping_rem(BSIZE as u32) as isize),
                user_src,
                src,
                m as _,
            )
            .is_err()
            {
                brelease(&mut *bp);
                break;
            } else {
                fs().log_write(bp);
                brelease(&mut *bp);
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

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub unsafe fn dirlookup(&mut self, name: &FileName) -> Result<(*mut Inode, u32), ()> {
        let mut de: Dirent = Default::default();
        assert_eq!(self.typ, T_DIR, "dirlookup not DIR");
        for off in (0..self.size).step_by(DIRENT_SIZE) {
            de.read_entry(self, off, "dirlookup read");
            if de.inum != 0 && name == de.get_name() {
                // entry matches path element
                return Ok((iget(self.ptr.dev, de.inum as u32), off));
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
            addr = self.addrs[bn];
            if addr == 0 {
                addr = balloc(self.ptr.dev);
                self.addrs[bn] = addr
            }
            return addr;
        }
        bn = (bn).wrapping_sub(NDIRECT);

        assert!(bn < NINDIRECT, "bmap: out of range");
        // Load indirect block, allocating if necessary.
        addr = self.addrs[NDIRECT];
        if addr == 0 {
            addr = balloc(self.ptr.dev);
            self.addrs[NDIRECT] = addr
        }
        let bp: *mut Buf = Buf::read(self.ptr.dev, addr);
        let a: *mut u32 = (*bp).inner.data.as_mut_ptr() as *mut u32;
        addr = *a.add(bn);
        if addr == 0 {
            addr = balloc(self.ptr.dev);
            *a.add(bn) = addr;
            fs().log_write(bp);
        }
        brelease(&mut *bp);
        addr
    }

    /// Is the directory dp empty except for "." and ".." ?
    pub unsafe fn isdirempty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.size).step_by(DIRENT_SIZE) {
            let bytes_read = self.read(
                0,
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
    pub unsafe fn lock(&self) -> InodeGuard<'_> {
        assert!(self.ref_0 >= 1, "Inode::lock");
        let mut guard = self.inner.lock();
        if !guard.valid {
            let bp: *mut Buf = Buf::read(self.dev, fs().superblock.iblock(self.inum));
            let dip: *mut Dinode = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
                .add((self.inum as usize).wrapping_rem(IPB));
            guard.typ = (*dip).typ;
            guard.major = (*dip).major as u16;
            guard.minor = (*dip).minor as u16;
            guard.nlink = (*dip).nlink;
            guard.size = (*dip).size;
            guard.addrs.copy_from_slice(&(*dip).addrs);
            brelease(&mut *bp);
            guard.valid = true;
            assert_ne!(guard.typ, T_NONE, "Inode::lock: no type");
        };
        InodeGuard::new(guard, self)
    }

    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode cache entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    #[allow(clippy::cast_ref_to_mut)]
    pub unsafe fn put(&self) {
        let mut inode = ICACHE.lock();

        if self.ref_0 == 1
            && self.inner.get_mut_unchecked().valid
            && self.inner.get_mut_unchecked().nlink == 0
        {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = self.lock();

            drop(inode);

            ip.itrunc();
            ip.typ = 0;
            ip.update();
            ip.valid = false;

            drop(ip);

            inode = ICACHE.lock();
        }
        //TODO : Use better code
        *(&self.ref_0 as *const _ as *mut i32) -= 1;
        drop(inode);
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by  giving it type type.
    /// Returns an unlocked but allocated and referenced inode.
    pub unsafe fn alloc(dev: u32, typ: i16) -> *mut Inode {
        for inum in 1..fs().superblock.ninodes {
            let bp = Buf::read(dev, fs().superblock.iblock(inum));
            let dip = ((*bp).inner.data.as_mut_ptr() as *mut Dinode)
                .add((inum as usize).wrapping_rem(IPB));

            // a free inode
            if (*dip).typ == 0 {
                ptr::write_bytes(dip, 0, 1);
                (*dip).typ = typ;

                // mark it allocated on the disk
                fs().log_write(bp);
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
            inner: SleeplockWIP::new(
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
