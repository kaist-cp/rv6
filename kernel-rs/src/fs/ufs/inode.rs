//! Inodes.
//!
//! An inode describes a single unnamed file.
//! The inode disk structure holds metadata: the file's type,
//! its size, the number of links referring to it, and the
//! list of blocks holding the file's content.
//!
//! The inodes are laid out sequentially on disk at
//! kernel.fs().superblock.startinode. Each inode has a number, indicating its
//! position on the disk.
//!
//! The kernel keeps a table of in-use inodes in memory
//! to provide a place for synchronizing access
//! to inodes used by multiple processes. The in-memory
//! inodes include book-keeping information that is
//! not stored on disk: ip->ref and ip->valid.
//!
//! An inode and its in-memory representation go through a
//! sequence of states before they can be used by the
//! rest of the file system code.
//!
//! * Allocation: an inode is allocated if its type (on disk)
//!   is non-zero. Inode::alloc() allocates, and Inode::put() frees if
//!   the reference and link counts have fallen to zero.
//!
//! * Referencing in table: an entry in the inode table
//!   is free if ip->ref is zero. Otherwise ip->ref tracks
//!   the number of in-memory pointers to the entry (open
//!   files and current directories). iget() finds or
//!   creates a cache entry and increments its ref; Inode::put()
//!   decrements ref.
//!
//! * Valid: the information (type, size, &c) in an inode
//!   table entry is only correct when ip->valid is 1.
//!   Inode::lock() reads the inode from
//!   the disk and sets ip->valid, while Inode::put() clears
//!   ip->valid if ip->ref has fallen to zero.
//!
//! * Locked: file system code may only examine and modify
//!   the information in an inode and its content if it
//!   has first locked the inode.
//!
//! Thus a typical sequence is:
//!   ip = iget(dev, inum)
//!   (*ip).lock()
//!   ... examine and modify ip->xxx ...
//!   (*ip).unlock()
//!   (*ip).put()
//!
//! Inode::lock() is separate from iget() so that system calls can
//! get a long-term reference to an inode (as for an open file)
//! and only lock it for short periods (e.g., in read()).
//! The separation also helps avoid deadlock and races during
//! pathname lookup. iget() increments ip->ref so that the inode
//! stays in the table and pointers to it remain valid.
//!
//! Many internal file system functions expect the caller to
//! have locked the inodes involved; this lets callers create
//! multi-step atomic operations.
//!
//! The kernel.itable.lock spin-lock protects the allocation of itable
//! entries. Since ip->ref indicates whether an entry is free,
//! and ip->dev and ip->inum indicate which i-node an entry
//! holds, one must hold kernel.itable.lock while using any of those fields.
//!
//! An ip->lock sleep-lock protects all ip-> fields other than ref,
//! dev, and inum.  One must hold ip->lock in order to
//! read or write that inode's ip->valid, ip->size, ip->type, &c.

use core::{
    iter::StepBy,
    mem,
    ops::{Deref, Range},
    ptr,
};

use static_assertions::const_assert;
use zerocopy::{AsBytes, FromBytes};

use super::{FileName, Path, Stat, UfsTx, IPB, MAXFILE, NDIRECT, NINDIRECT, ROOTINO};
use crate::{
    arch::addr::UVAddr,
    arena::{Arena, ArenaObject, ArrayArena},
    bio::BufData,
    fs::{Inode, InodeGuard, InodeType, Itable, RcInode},
    hal::hal,
    lock::{SleepLock, SpinLock},
    param::ROOTDEV,
    param::{BSIZE, NINODE},
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// dirent size
pub const DIRENT_SIZE: usize = mem::size_of::<Dirent>();

#[derive(Copy, Clone, PartialEq, Debug)]
#[repr(i16)]
pub enum DInodeType {
    None,
    Dir,
    File,
    Device,
}

pub struct InodeInner {
    /// inode has been read from disk?
    pub valid: bool,
    /// copy of disk inode
    pub typ: InodeType,
    pub nlink: i16,
    pub size: u32,
    pub addr_direct: [u32; NDIRECT],
    pub addr_indirect: u32,
}

/// On-disk inode structure
/// Both the kernel and user programs use this header file.
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct Dinode {
    /// File type
    typ: DInodeType,

    /// Major device number (T_DEVICE only)
    major: u16,

    /// Minor device number (T_DEVICE only)
    minor: u16,

    /// Number of links to inode in file system
    nlink: i16,

    /// Size of file (bytes)
    size: u32,

    /// Direct data block addresses
    addr_direct: [u32; NDIRECT],

    /// Indirect data block address
    addr_indirect: u32,
}

#[repr(C)]
#[derive(Default, AsBytes, FromBytes)]
pub struct Dirent {
    pub inum: u16,
    name: [u8; DIRSIZ],
}

impl Dirent {
    fn new(
        ip: &mut InodeGuard<'_, InodeInner>,
        off: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<Dirent, ()> {
        let mut dirent = Dirent::default();
        ip.read_kernel(&mut dirent, off, ctx)?;
        Ok(dirent)
    }

    /// Fill in name. If name is shorter than DIRSIZ, NUL character is appended as
    /// terminator.
    ///
    /// `name` must not contain NUL characters, but this is not a safety invariant.
    fn set_name(&mut self, name: &FileName<{ DIRSIZ }>) {
        let name = name.as_bytes();
        if name.len() == DIRSIZ {
            self.name.copy_from_slice(name);
        } else {
            self.name[..name.len()].copy_from_slice(name);
            self.name[name.len()] = 0;
        }
    }

    /// Returns slice which exactly contains `name`.
    ///
    /// It contains no NUL characters.
    fn get_name(&self) -> &FileName<{ DIRSIZ }> {
        let len = self.name.iter().position(|ch| *ch == 0).unwrap_or(DIRSIZ);
        // SAFETY: self.name[..len] doesn't contain '\0', and len must be <= DIRSIZ.
        unsafe { FileName::from_bytes(&self.name[..len]) }
    }
}

struct DirentIter<'id, 's, 't> {
    guard: &'s mut InodeGuard<'t, InodeInner>,
    iter: StepBy<Range<u32>>,
    ctx: &'s KernelCtx<'id, 's>,
}

impl Iterator for DirentIter<'_, '_, '_> {
    type Item = (Dirent, u32);

    fn next(&mut self) -> Option<Self::Item> {
        let off = self.iter.next()?;
        let dirent = Dirent::new(self.guard, off, self.ctx).expect("DirentIter");
        Some((dirent, off))
    }
}

impl<'t> InodeGuard<'t, InodeInner> {
    fn iter_dirents<'id, 's>(&'s mut self, ctx: &'s KernelCtx<'id, 's>) -> DirentIter<'id, 's, 't> {
        let iter = (0..self.deref_inner().size).step_by(DIRENT_SIZE);
        DirentIter {
            guard: self,
            iter,
            ctx,
        }
    }
}

// Directories
impl InodeGuard<'_, InodeInner> {
    /// Write a new directory entry (name, inum) into the directory dp.
    pub fn dirlink(
        &mut self,
        name: &FileName<{ DIRSIZ }>,
        inum: u32,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        // Check that name is not present.
        if let Ok((ip, _)) = self.dirlookup(name, ctx) {
            ip.free((tx, ctx));
            return Err(());
        };

        // Look for an empty Dirent.
        let (mut de, off) = self
            .iter_dirents(ctx)
            .find(|(de, _)| de.inum == 0)
            .unwrap_or((Default::default(), self.deref_inner().size));
        de.inum = inum as _;
        de.set_name(name);
        self.write_kernel(&de, off, tx, ctx).expect("dirlink");
        Ok(())
    }

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub fn dirlookup(
        &mut self,
        name: &FileName<{ DIRSIZ }>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<InodeInner>, u32), ()> {
        assert_eq!(self.deref_inner().typ, InodeType::Dir, "dirlookup not DIR");

        self.iter_dirents(ctx)
            .find(|(de, _)| de.inum != 0 && de.get_name() == name)
            .map(|(de, off)| {
                (
                    ctx.kernel()
                        .fs()
                        .itable()
                        .get_inode(self.dev, de.inum as u32),
                    off,
                )
            })
            .ok_or(())
    }
}

impl InodeGuard<'_, InodeInner> {
    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk.
    pub fn update(&self, tx: &UfsTx<'_>, ctx: &KernelCtx<'_, '_>) {
        let mut bp = hal().disk().read(
            self.dev,
            ctx.kernel().fs().superblock().iblock(self.inum),
            ctx,
        );

        const_assert!(IPB <= mem::size_of::<BufData>() / mem::size_of::<Dinode>());
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<Dinode>() == 0);
        // SAFETY:
        // * dip is aligned properly.
        // * dip is inside bp.data.
        // * dip will not be read.
        let dip = unsafe {
            &mut *(bp.deref_inner_mut().data.as_mut_ptr() as *mut Dinode)
                .add(self.inum as usize % IPB)
        };

        let inner = self.deref_inner();
        match inner.typ {
            InodeType::Device { major, minor } => {
                dip.typ = DInodeType::Device;
                dip.major = major;
                dip.minor = minor;
            }
            InodeType::None => {
                dip.typ = DInodeType::None;
                dip.major = 0;
                dip.minor = 0;
            }
            InodeType::Dir => {
                dip.typ = DInodeType::Dir;
                dip.major = 0;
                dip.minor = 0;
            }
            InodeType::File => {
                dip.typ = DInodeType::File;
                dip.major = 0;
                dip.minor = 0;
            }
        }

        (*dip).nlink = inner.nlink;
        (*dip).size = inner.size;
        (*dip).addr_direct.copy_from_slice(&inner.addr_direct);
        (*dip).addr_indirect = inner.addr_indirect;
        tx.write(bp, ctx);
    }

    /// Truncate inode (discard contents).
    /// This function is called with Inode's lock is held.
    pub fn itrunc(&mut self, tx: &UfsTx<'_>, ctx: &KernelCtx<'_, '_>) {
        let dev = self.dev;
        for addr in &mut self.deref_inner_mut().addr_direct {
            if *addr != 0 {
                tx.bfree(dev, *addr, ctx);
                *addr = 0;
            }
        }

        if self.deref_inner().addr_indirect != 0 {
            let mut bp = hal()
                .disk()
                .read(dev, self.deref_inner().addr_indirect, ctx);
            // SAFETY: u32 does not have internal structure.
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "itrunc: Buf data unaligned");
            for a in data {
                if *a != 0 {
                    tx.bfree(dev, *a, ctx);
                }
            }
            bp.free(ctx);
            tx.bfree(dev, self.deref_inner().addr_indirect, ctx);
            self.deref_inner_mut().addr_indirect = 0
        }

        self.deref_inner_mut().size = 0;
        self.update(tx, ctx);
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return Ok(()) on success, Err(()) on failure.
    pub fn read_kernel<T: AsBytes + FromBytes>(
        &mut self,
        dst: &mut T,
        off: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let bytes = self.read_bytes_kernel(dst.as_bytes_mut(), off, ctx);
        if bytes == mem::size_of::<T>() {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return the number of bytes copied.
    pub fn read_bytes_kernel(
        &mut self,
        dst: &mut [u8],
        off: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> usize {
        self.read_internal(
            off,
            dst.len() as u32,
            |off, src, _| {
                dst[off as usize..off as usize + src.len()].clone_from_slice(src);
                Ok(())
            },
            ctx,
        )
        .expect("read: should never fail")
    }

    /// Copy data into virtual address `dst` of the current process by `n` bytes
    /// from the content of inode at offset `off`.
    /// Returns Ok(number of bytes copied) on success, Err(()) on failure due to
    /// accessing an invalid virtual address.
    pub fn read_user(
        &mut self,
        dst: UVAddr,
        off: u32,
        n: u32,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        self.read_internal(
            off,
            n,
            |off, src, ctx| {
                ctx.proc_mut()
                    .memory_mut()
                    .copy_out_bytes(dst + off as usize, src)
            },
            ctx,
        )
    }

    /// Read data from inode.
    ///
    /// `f` takes an offset and a slice as arguments. `f(off, src, ctx)` should copy
    /// the content of `src` to the interval beginning at `off`th byte of the
    /// destination, which the caller of this method knows.
    // This method takes a function as an argument, because writing to kernel
    // memory and user memory are very different from each other. Writing to a
    // consecutive region in kernel memory can be done at once by simple memcpy.
    // However, writing to user memory needs page table accesses since a single
    // consecutive region in user memory may split into several pages in
    // physical memory.
    #[inline]
    fn read_internal<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &[u8], &mut K) -> Result<(), ()>,
    >(
        &mut self,
        mut off: u32,
        mut n: u32,
        mut f: F,
        mut k: K,
    ) -> Result<usize, ()> {
        let inner = self.deref_inner();
        if off > inner.size || off.wrapping_add(n) < off {
            return Ok(0);
        }
        if off + n > inner.size {
            n = inner.size - off;
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = hal()
                .disk()
                .read(self.dev, self.bmap(off as usize / BSIZE, &k), &k);
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            let res = f(tot, &bp.deref_inner().data[begin..end], &mut k);
            bp.free(&k);
            res?;
            tot += m;
            off += m;
        }
        Ok(tot as usize)
    }

    /// Copy data from `src` into the inode at offset `off`.
    /// Return Ok(()) on success, Err(()) on failure.
    pub fn write_kernel<T: AsBytes>(
        &mut self,
        src: &T,
        off: u32,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let bytes = self.write_bytes_kernel(src.as_bytes(), off, tx, ctx)?;
        if bytes == mem::size_of::<T>() {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Copy data from `src` into the inode at offset `off`.
    /// Returns Ok(number of bytes copied) on success, Err(()) on failure.
    pub fn write_bytes_kernel(
        &mut self,
        src: &[u8],
        off: u32,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        self.write_internal(
            off,
            src.len() as u32,
            |off, dst, _| {
                dst.clone_from_slice(&src[off as usize..off as usize + src.len()]);
                Ok(())
            },
            tx,
            ctx,
        )
    }

    /// Copy data from virtual address `src` of the current process by `n` bytes
    /// into the inode at offset `off`.
    /// Returns Ok(number of bytes copied) on success, Err(()) on failure.
    pub fn write_user(
        &mut self,
        src: UVAddr,
        off: u32,
        n: u32,
        ctx: &mut KernelCtx<'_, '_>,
        tx: &UfsTx<'_>,
    ) -> Result<usize, ()> {
        self.write_internal(
            off,
            n,
            |off, dst, ctx| {
                ctx.proc_mut()
                    .memory_mut()
                    .copy_in_bytes(dst, src + off as usize)
            },
            tx,
            ctx,
        )
    }

    /// Write data to inode. Returns the number of bytes successfully written.
    /// If the return value is less than the requested n, there was an error of
    /// some kind.
    ///
    /// `f` takes an offset and a slice as arguments. `f(off, dst)` should copy
    /// the content beginning at the `off`th byte of the source, which the
    /// caller of this method knows, to `dst`.
    // This method takes a function as an argument, because reading kernel
    // memory and user memory are very different from each other. Reading a
    // consecutive region in kernel memory can be done at once by simple memcpy.
    // However, reading user memory needs page table accesses since a single
    // consecutive region in user memory may split into several pages in
    // physical memory.
    #[inline]
    fn write_internal<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &mut [u8], &mut K) -> Result<(), ()>,
    >(
        &mut self,
        mut off: u32,
        n: u32,
        mut f: F,
        tx: &UfsTx<'_>,
        mut k: K,
    ) -> Result<usize, ()> {
        if off > self.deref_inner().size {
            return Err(());
        }
        if off.checked_add(n).ok_or(())? as usize > MAXFILE * BSIZE {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = hal().disk().read(
                self.dev,
                self.bmap_or_alloc(off as usize / BSIZE, tx, &k),
                &k,
            );
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            if f(tot, &mut bp.deref_inner_mut().data[begin..end], &mut k).is_ok() {
                tx.write(bp, &k);
            } else {
                bp.free(&k);
                break;
            }
            tot += m;
            off += m;
        }

        if off > self.deref_inner().size {
            self.deref_inner_mut().size = off;
        }

        // Write the i-node back to disk even if the size didn't change
        // because the loop above might have called bmap() and added a new
        // block to self->addrs[].
        self.update(tx, &k);
        Ok(tot as usize)
    }

    /// Inode content
    ///
    /// The content (data) associated with each inode is stored
    /// in blocks on the disk. The first NDIRECT block numbers
    /// are listed in self->addrs[].  The next NINDIRECT blocks are
    /// listed in block self->addr_indirect.
    /// Return the disk block address of the nth block in inode self.
    /// If there is no such block, bmap allocates one.
    fn bmap_or_alloc(&mut self, bn: usize, tx: &UfsTx<'_>, ctx: &KernelCtx<'_, '_>) -> u32 {
        self.bmap_internal(bn, Some(tx), ctx)
    }

    fn bmap(&mut self, bn: usize, ctx: &KernelCtx<'_, '_>) -> u32 {
        self.bmap_internal(bn, None, ctx)
    }

    fn bmap_internal(
        &mut self,
        bn: usize,
        tx_opt: Option<&UfsTx<'_>>,
        ctx: &KernelCtx<'_, '_>,
    ) -> u32 {
        let inner = self.deref_inner();

        if bn < NDIRECT {
            let mut addr = inner.addr_direct[bn];
            if addr == 0 {
                addr = tx_opt.expect("bmap: out of range").balloc(self.dev, ctx);
                self.deref_inner_mut().addr_direct[bn] = addr;
            }
            addr
        } else {
            let bn = bn - NDIRECT;
            assert!(bn < NINDIRECT, "bmap: out of range");

            let mut indirect = inner.addr_indirect;
            if indirect == 0 {
                indirect = tx_opt.expect("bmap: out of range").balloc(self.dev, ctx);
                self.deref_inner_mut().addr_indirect = indirect;
            }

            let mut bp = hal().disk().read(self.dev, indirect, ctx);
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "bmap: Buf data unaligned");
            let mut addr = data[bn];
            if addr == 0 {
                let tx = tx_opt.expect("bmap: out of range");
                addr = tx.balloc(self.dev, ctx);
                data[bn] = addr;
                tx.write(bp, ctx);
            } else {
                bp.free(ctx);
            }
            addr
        }
    }

    /// Is the directory dp empty except for "." and ".." ?
    pub fn is_dir_empty(&mut self, ctx: &KernelCtx<'_, '_>) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.deref_inner().size).step_by(DIRENT_SIZE) {
            self.read_kernel(&mut de, off, ctx)
                .expect("is_dir_empty: read_kernel");
            if de.inum != 0 {
                return false;
            }
        }
        true
    }
}

impl const Default for Inode<InodeInner> {
    fn default() -> Self {
        Self::new()
    }
}

impl ArenaObject for Inode<InodeInner> {
    type Ctx<'a, 'id: 'a> = (&'a UfsTx<'a>, &'a KernelCtx<'id, 'a>);

    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode table entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    fn finalize<'a, 'id: 'a, A: Arena>(&mut self, ctx: Self::Ctx<'a, 'id>) {
        let (tx, ctx) = ctx;
        if self.inner.get_mut().valid && self.inner.get_mut().nlink == 0 {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = self.lock(ctx);

            ip.itrunc(tx, ctx);
            ip.deref_inner_mut().typ = InodeType::None;
            ip.update(tx, ctx);
            ip.deref_inner_mut().valid = false;

            ip.free(ctx);
        }
    }
}

impl Inode<InodeInner> {
    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub fn lock(&self, ctx: &KernelCtx<'_, '_>) -> InodeGuard<'_, InodeInner> {
        let mut guard = self.inner.lock(ctx);
        if !guard.valid {
            let mut bp = hal().disk().read(
                self.dev,
                ctx.kernel().fs().superblock().iblock(self.inum),
                ctx,
            );

            // SAFETY: dip is inside bp.data.
            let dip = unsafe {
                (bp.deref_inner_mut().data.as_mut_ptr() as *mut Dinode)
                    .add(self.inum as usize % IPB)
            };
            // SAFETY: i16 does not have internal structure.
            let t = unsafe { *(dip as *const i16) };
            // If t >= #(variants of DInodeType), UB will happen when we read dip.typ.
            assert!(t < core::mem::variant_count::<DInodeType>() as i16);
            // SAFETY: dip is aligned properly and t < #(variants of DInodeType).
            let dip = unsafe { &mut *dip };

            match dip.typ {
                DInodeType::None => guard.typ = InodeType::None,
                DInodeType::Dir => guard.typ = InodeType::Dir,
                DInodeType::File => guard.typ = InodeType::File,
                DInodeType::Device => {
                    guard.typ = InodeType::Device {
                        major: dip.major,
                        minor: dip.minor,
                    }
                }
            }
            guard.nlink = dip.nlink;
            guard.size = dip.size;
            guard.addr_direct.copy_from_slice(&dip.addr_direct);
            guard.addr_indirect = dip.addr_indirect;
            bp.free(ctx);
            guard.valid = true;
            assert_ne!(guard.typ, InodeType::None, "Inode::lock: no type");
        };
        mem::forget(guard);
        InodeGuard { inode: self }
    }

    pub const fn new() -> Self {
        Self {
            dev: 0,
            inum: 0,
            inner: SleepLock::new(
                "inode",
                InodeInner {
                    valid: false,
                    typ: InodeType::None,
                    nlink: 0,
                    size: 0,
                    addr_direct: [0; NDIRECT],
                    addr_indirect: 0,
                },
            ),
        }
    }

    /// Copy stat information from inode.
    pub fn stat(&self, ctx: &KernelCtx<'_, '_>) -> Stat {
        let inner = self.inner.lock(ctx);
        let st = Stat {
            dev: self.dev as i32,
            ino: self.inum,
            typ: match inner.typ {
                InodeType::None => 0,
                InodeType::Dir => 1,
                InodeType::File => 2,
                InodeType::Device { .. } => 3,
            },
            nlink: inner.nlink,
            _padding: 0,
            size: inner.size as usize,
        };
        inner.free(ctx);
        st
    }
}

impl Itable<InodeInner> {
    pub const fn new_itable() -> Self {
        SpinLock::new("ITABLE", ArrayArena::<Inode<InodeInner>, NINODE>::new())
    }

    /// Find the inode with number inum on device dev
    /// and return the in-memory copy. Does not lock
    /// the inode and does not read it from disk.
    pub fn get_inode(self: StrongPin<'_, Self>, dev: u32, inum: u32) -> RcInode<InodeInner> {
        self.find_or_alloc(
            |inode| inode.dev == dev && inode.inum == inum,
            |inode| {
                inode.dev = dev;
                inode.inum = inum;
                inode.inner.get_mut().valid = false;
            },
        )
        .expect("[Itable::get_inode] no inodes")
    }

    /// Allocate an inode on device dev.
    /// Mark it as allocated by giving it type.
    /// Returns an unlocked but allocated and referenced inode.
    pub fn alloc_inode(
        self: StrongPin<'_, Self>,
        dev: u32,
        typ: InodeType,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> RcInode<InodeInner> {
        for inum in 1..ctx.kernel().fs().superblock().ninodes {
            let mut bp = hal()
                .disk()
                .read(dev, ctx.kernel().fs().superblock().iblock(inum), ctx);

            const_assert!(IPB <= mem::size_of::<BufData>() / mem::size_of::<Dinode>());
            const_assert!(mem::align_of::<BufData>() % mem::align_of::<Dinode>() == 0);
            // SAFETY: dip is inside bp.data.
            let dip = unsafe {
                (bp.deref_inner_mut().data.as_mut_ptr() as *mut Dinode).add(inum as usize % IPB)
            };
            // SAFETY: i16 does not have internal structure.
            let t = unsafe { *(dip as *const i16) };
            // If t >= #(variants of DInodeType), UB will happen when we read dip.typ.
            assert!(t < core::mem::variant_count::<DInodeType>() as i16);
            // SAFETY: dip is aligned properly and t < #(variants of DInodeType).
            let dip = unsafe { &mut *dip };

            // a free inode
            if dip.typ == DInodeType::None {
                unsafe { ptr::write_bytes(dip as _, 0, 1) };
                match typ {
                    InodeType::None => dip.typ = DInodeType::None,
                    InodeType::Dir => dip.typ = DInodeType::Dir,
                    InodeType::File => dip.typ = DInodeType::File,
                    InodeType::Device { major, minor } => {
                        dip.typ = DInodeType::Device;
                        dip.major = major;
                        dip.minor = minor
                    }
                }

                // mark it allocated on the disk
                tx.write(bp, ctx);
                return self.get_inode(dev, inum);
            } else {
                bp.free(ctx);
            }
        }
        panic!("[Itable::alloc_inode] no inodes");
    }

    pub fn root(self: StrongPin<'_, Self>) -> RcInode<InodeInner> {
        self.get_inode(ROOTDEV, ROOTINO)
    }

    pub fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &UfsTx<'_>,
        proc: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<InodeInner>, ()> {
        Ok(self.namex(path, false, tx, proc)?.0)
    }

    pub fn nameiparent<'s>(
        self: StrongPin<'_, Self>,
        path: &'s Path,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<InodeInner>, &'s FileName<{ DIRSIZ }>), ()> {
        let (ip, name_in_path) = self.namex(path, true, tx, ctx)?;
        let name_in_path = name_in_path.ok_or(())?;
        Ok((ip, name_in_path))
    }

    fn namex<'s>(
        self: StrongPin<'_, Self>,
        mut path: &'s Path,
        parent: bool,
        tx: &UfsTx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<InodeInner>, Option<&'s FileName<{ DIRSIZ }>>), ()> {
        let mut ptr = if path.is_absolute() {
            self.root()
        } else {
            ctx.proc().cwd().clone()
        };

        while let Some((new_path, name)) = path.skipelem() {
            path = new_path;

            let mut ip = ptr.lock(ctx);
            if ip.deref_inner().typ != InodeType::Dir {
                ip.free(ctx);
                ptr.free((tx, ctx));
                return Err(());
            }
            if parent && path.is_empty_string() {
                // Stop one level early.
                ip.free(ctx);
                return Ok((ptr, Some(name)));
            }
            let next = ip.dirlookup(name, ctx);
            ip.free(ctx);
            ptr.free((tx, ctx));
            ptr = next?.0
        }
        if parent {
            ptr.free((tx, ctx));
            return Err(());
        }
        Ok((ptr, None))
    }
}
