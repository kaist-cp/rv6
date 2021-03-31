//! Inodes.
//!
//! An inode describes a single unnamed file.
//! The inode disk structure holds metadata: the file's type,
//! its size, the number of links referring to it, and the
//! list of blocks holding the file's content.
//!
//! The inodes are laid out sequentially on disk at
//! kernel().file_system.superblock.startinode. Each inode has a number, indicating its
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
//! The kernel().itable.lock spin-lock protects the allocation of itable
//! entries. Since ip->ref indicates whether an entry is free,
//! and ip->dev and ip->inum indicate which i-node an entry
//! holds, one must hold kernel().itable.lock while using any of those fields.
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

use super::{FileName, IPB, MAXFILE, NDIRECT, NINDIRECT};
use crate::{
    arena::{Arena, ArenaObject, ArrayArena, Rc},
    bio::BufData,
    fs::{FsTransaction, Path, ROOTINO},
    kernel::kernel_builder,
    lock::{Sleeplock, Spinlock},
    param::ROOTDEV,
    param::{BSIZE, NINODE},
    proc::CurrentProc,
    stat::Stat,
    vm::UVAddr,
};

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// dirent size
pub const DIRENT_SIZE: usize = mem::size_of::<Dirent>();

#[derive(Copy, Clone, PartialEq, Debug)]
#[repr(i16)]
pub enum InodeType {
    None,
    Dir,
    File,
    Device { major: u16, minor: u16 },
}
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

/// in-memory copy of an inode
pub struct Inode {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    pub inner: Sleeplock<InodeInner>,
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

pub type Itable = Spinlock<ArrayArena<Inode, NINODE>>;

/// A reference counted smart pointer to an `Inode`.
pub type RcInode = Rc<Itable>;

/// InodeGuard implies that `Sleeplock<InodeInner>` is held by current thread.
///
/// # Safety
///
/// `inode.inner` is locked.
// Every disk write operation must happen inside a transaction. Reading an
// opened file does not write anything on disk in any matter and thus does
// not need to happen inside a transaction. At the same time, it requires
// an InodeGuard. Therefore, InodeGuard does not have a FsTransaction field.
// Instead, every method that needs to be inside a transaction explicitly
// takes a FsTransaction value as an argument.
// https://github.com/kaist-cp/rv6/issues/328
pub struct InodeGuard<'a> {
    pub inode: &'a Inode,
}

#[derive(Default)]
pub struct Dirent {
    pub inum: u16,
    name: [u8; DIRSIZ],
}

impl Dirent {
    fn new(ip: &mut InodeGuard<'_>, off: u32) -> Result<Dirent, ()> {
        let mut dirent = Dirent::default();
        // SAFETY: Dirent can be safely transmuted to [u8; _], as it
        // contains only u16 and u8's, which do not have internal structures.
        unsafe { ip.read_kernel(&mut dirent, off) }?;
        Ok(dirent)
    }

    /// Fill in name. If name is shorter than DIRSIZ, NUL character is appended as
    /// terminator.
    ///
    /// `name` must not contain NUL characters, but this is not a safety invariant.
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
        // SAFETY: self.name[..len] doesn't contain '\0', and len must be <= DIRSIZ.
        unsafe { FileName::from_bytes(&self.name[..len]) }
    }
}

struct DirentIter<'s, 't> {
    guard: &'s mut InodeGuard<'t>,
    iter: StepBy<Range<u32>>,
}

impl Iterator for DirentIter<'_, '_> {
    type Item = (Dirent, u32);

    fn next(&mut self) -> Option<Self::Item> {
        let off = self.iter.next()?;
        let dirent = Dirent::new(self.guard, off).expect("DirentIter");
        Some((dirent, off))
    }
}

impl<'t> InodeGuard<'t> {
    fn iter_dirents<'s>(&'s mut self) -> DirentIter<'s, 't> {
        let iter = (0..self.deref_inner().size).step_by(DIRENT_SIZE);
        DirentIter { guard: self, iter }
    }
}

impl Deref for InodeGuard<'_> {
    type Target = Inode;

    fn deref(&self) -> &Self::Target {
        self.inode
    }
}

impl InodeGuard<'_> {
    pub fn deref_inner(&self) -> &InodeInner {
        // SAFETY: self.inner is locked.
        unsafe { &*self.inner.get_mut_raw() }
    }

    pub fn deref_inner_mut(&mut self) -> &mut InodeInner {
        // SAFETY: self.inner is locked and &mut self is exclusive.
        unsafe { &mut *self.inner.get_mut_raw() }
    }
}

/// Unlock and put the given inode.
impl Drop for InodeGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: self will be dropped.
        unsafe { self.inner.unlock() };
    }
}

// Directories
impl InodeGuard<'_> {
    /// Write a new directory entry (name, inum) into the directory dp.
    pub fn dirlink(
        &mut self,
        name: &FileName,
        inum: u32,
        tx: &FsTransaction<'_>,
        itable: &Itable,
    ) -> Result<(), ()> {
        // Check that name is not present.
        if let Ok((_ip, _)) = self.dirlookup(name, itable) {
            return Err(());
        };

        // Look for an empty Dirent.
        let (mut de, off) = self
            .iter_dirents()
            .find(|(de, _)| de.inum == 0)
            .unwrap_or((Default::default(), self.deref_inner().size));
        de.inum = inum as _;
        de.set_name(name);
        self.write_kernel(&de, off, tx).expect("dirlink");
        Ok(())
    }

    /// Look for a directory entry in a directory.
    /// If found, return the entry and byte offset of entry.
    pub fn dirlookup<'a>(
        &mut self,
        name: &FileName,
        itable: &'a Itable,
    ) -> Result<(RcInode, u32), ()> {
        assert_eq!(self.deref_inner().typ, InodeType::Dir, "dirlookup not DIR");

        self.iter_dirents()
            .find(|(de, _)| de.inum != 0 && de.get_name() == name)
            .map(|(de, off)| (itable.get_inode(self.dev, de.inum as u32), off))
            .ok_or(())
    }
}

impl InodeGuard<'_> {
    /// Copy a modified in-memory inode to disk.
    /// Must be called after every change to an ip->xxx field
    /// that lives on disk.
    pub fn update(&self, tx: &FsTransaction<'_>) {
        // TODO: remove kernel_builder()
        let mut bp = kernel_builder().file_system.log.disk.read(
            self.dev,
            // TODO: remove kernel_builder()
            kernel_builder().file_system.superblock().iblock(self.inum),
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
        tx.write(bp);
    }

    /// Truncate inode (discard contents).
    /// This function is called with Inode's lock is held.
    pub fn itrunc(&mut self, tx: &FsTransaction<'_>) {
        let dev = self.dev;
        for addr in &mut self.deref_inner_mut().addr_direct {
            if *addr != 0 {
                tx.bfree(dev, *addr);
                *addr = 0;
            }
        }

        if self.deref_inner().addr_indirect != 0 {
            // TODO: remove kernel_builder()
            let mut bp = kernel_builder()
                .file_system
                .log
                .disk
                .read(dev, self.deref_inner().addr_indirect);
            // SAFETY: u32 does not have internal structure.
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "itrunc: Buf data unaligned");
            for a in data {
                if *a != 0 {
                    tx.bfree(dev, *a);
                }
            }
            drop(bp);
            tx.bfree(dev, self.deref_inner().addr_indirect);
            self.deref_inner_mut().addr_indirect = 0
        }

        self.deref_inner_mut().size = 0;
        self.update(tx);
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return Ok(()) on success, Err(()) on failure.
    ///
    /// # Safety
    ///
    /// `T` can be safely `transmute`d to `[u8; size_of::<T>()]`.
    pub unsafe fn read_kernel<T>(&mut self, dst: &mut T, off: u32) -> Result<(), ()> {
        let bytes = self.read_bytes_kernel(
            // SAFETY: the safety assumption of this method.
            unsafe { core::slice::from_raw_parts_mut(dst as *mut _ as _, mem::size_of::<T>()) },
            off,
        );
        if bytes == mem::size_of::<T>() {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Copy data into `dst` from the content of inode at offset `off`.
    /// Return the number of bytes copied.
    pub fn read_bytes_kernel(&mut self, dst: &mut [u8], off: u32) -> usize {
        self.read_internal(off, dst.len() as u32, |off, src| {
            dst[off as usize..off as usize + src.len()].clone_from_slice(src);
            Ok(())
        })
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
        proc: &mut CurrentProc<'_>,
    ) -> Result<usize, ()> {
        self.read_internal(off, n, |off, src| {
            proc.memory_mut().copy_out_bytes(dst + off as usize, src)
        })
    }

    /// Read data from inode.
    ///
    /// `f` takes an offset and a slice as arguments. `f(off, src)` should copy
    /// the content of `src` to the interval beginning at `off`th byte of the
    /// destination, which the caller of this method knows.
    // This method takes a function as an argument, because writing to kernel
    // memory and user memory are very different from each other. Writing to a
    // consecutive region in kernel memory can be done at once by simple memcpy.
    // However, writing to user memory needs page table accesses since a single
    // consecutive region in user memory may split into several pages in
    // physical memory.
    fn read_internal<F: FnMut(u32, &[u8]) -> Result<(), ()>>(
        &mut self,
        mut off: u32,
        mut n: u32,
        mut f: F,
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
            // TODO: remove kernel_builder()
            let bp = kernel_builder()
                .file_system
                .log
                .disk
                .read(self.dev, self.bmap(off as usize / BSIZE));
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            f(tot, &bp.deref_inner().data[begin..end])?;
            tot += m;
            off += m;
        }
        Ok(tot as usize)
    }

    /// Copy data from `src` into the inode at offset `off`.
    /// Return Ok(()) on success, Err(()) on failure.
    pub fn write_kernel<T>(&mut self, src: &T, off: u32, tx: &FsTransaction<'_>) -> Result<(), ()> {
        let bytes = self.write_bytes_kernel(
            // SAFETY: src is a valid reference to T and
            // u8 does not have any internal structure.
            unsafe { core::slice::from_raw_parts(src as *const _ as _, mem::size_of::<T>()) },
            off,
            tx,
        )?;
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
        tx: &FsTransaction<'_>,
    ) -> Result<usize, ()> {
        self.write_internal(
            off,
            src.len() as u32,
            |off, dst| {
                dst.clone_from_slice(&src[off as usize..off as usize + src.len()]);
                Ok(())
            },
            tx,
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
        proc: &mut CurrentProc<'_>,
        tx: &FsTransaction<'_>,
    ) -> Result<usize, ()> {
        self.write_internal(
            off,
            n,
            |off, dst| proc.memory_mut().copy_in_bytes(dst, src + off as usize),
            tx,
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
    fn write_internal<F: FnMut(u32, &mut [u8]) -> Result<(), ()>>(
        &mut self,
        mut off: u32,
        n: u32,
        mut f: F,
        tx: &FsTransaction<'_>,
    ) -> Result<usize, ()> {
        if off > self.deref_inner().size {
            return Err(());
        }
        if off.checked_add(n).ok_or(())? as usize > MAXFILE * BSIZE {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            // TODO: remove kernel_builder()
            let mut bp = kernel_builder()
                .file_system
                .log
                .disk
                .read(self.dev, self.bmap_or_alloc(off as usize / BSIZE, tx));
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            if f(tot, &mut bp.deref_inner_mut().data[begin..end]).is_err() {
                break;
            }
            tx.write(bp);
            tot += m;
            off += m;
        }

        if off > self.deref_inner().size {
            self.deref_inner_mut().size = off;
        }

        // Write the i-node back to disk even if the size didn't change
        // because the loop above might have called bmap() and added a new
        // block to self->addrs[].
        self.update(tx);
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
    fn bmap_or_alloc(&mut self, bn: usize, tx: &FsTransaction<'_>) -> u32 {
        self.bmap_internal(bn, Some(tx))
    }

    fn bmap(&mut self, bn: usize) -> u32 {
        self.bmap_internal(bn, None)
    }

    fn bmap_internal(&mut self, bn: usize, tx_opt: Option<&FsTransaction<'_>>) -> u32 {
        let inner = self.deref_inner();

        if bn < NDIRECT {
            let mut addr = inner.addr_direct[bn];
            if addr == 0 {
                addr = tx_opt.expect("bmap: out of range").balloc(self.dev);
                self.deref_inner_mut().addr_direct[bn] = addr;
            }
            addr
        } else {
            let bn = bn - NDIRECT;
            assert!(bn < NINDIRECT, "bmap: out of range");

            let mut indirect = inner.addr_indirect;
            if indirect == 0 {
                indirect = tx_opt.expect("bmap: out of range").balloc(self.dev);
                self.deref_inner_mut().addr_indirect = indirect;
            }

            // TODO: remove kernel_builder()
            let mut bp = kernel_builder()
                .file_system
                .log
                .disk
                .read(self.dev, indirect);
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "bmap: Buf data unaligned");
            let mut addr = data[bn];
            if addr == 0 {
                let tx = tx_opt.expect("bmap: out of range");
                addr = tx.balloc(self.dev);
                data[bn] = addr;
                tx.write(bp);
            }
            addr
        }
    }

    /// Is the directory dp empty except for "." and ".." ?
    pub fn is_dir_empty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.deref_inner().size).step_by(DIRENT_SIZE) {
            // SAFETY: Dirent can be safely transmuted to [u8; _], as it
            // contains only u16 and u8's, which do not have internal structures.
            unsafe { self.read_kernel(&mut de, off) }.expect("is_dir_empty: read_kernel");
            if de.inum != 0 {
                return false;
            }
        }
        true
    }
}

#[rustfmt::skip] // Need this if lower than rustfmt 1.4.34
impl const Default for Inode {
    fn default() -> Self {
        Self::zero()
    }
}

impl ArenaObject for Inode {
    /// Drop a reference to an in-memory inode.
    /// If that was the last reference, the inode table entry can
    /// be recycled.
    /// If that was the last reference and the inode has no links
    /// to it, free the inode (and its content) on disk.
    /// All calls to Inode::put() must be inside a transaction in
    /// case it has to free the inode.
    #[allow(clippy::cast_ref_to_mut)]
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>) {
        if self.inner.get_mut().valid && self.inner.get_mut().nlink == 0 {
            // inode has no links and no other references: truncate and free.

            // TODO(https://github.com/kaist-cp/rv6/issues/290)
            // Disk write operations must happen inside a transaction. However,
            // we cannot begin a new transaction here because beginning of a
            // transaction acquires a sleep lock while the spin lock of this
            // arena has been acquired before the invocation of this method.
            // To mitigate this problem, we make a fake transaction and pass
            // it as an argument for each disk write operation below. As a
            // transaction does not start here, any operation that can drop an
            // inode must begin a transaction even in the case that the
            // resulting FsTransaction value is never used. Such transactions
            // can be found in finalize in file.rs, sys_chdir in sysfile.rs,
            // close_files in proc.rs, and exec in exec.rs.
            let tx = mem::ManuallyDrop::new(FsTransaction {
                // TODO: remove kernel_builder()
                fs: &kernel_builder().file_system,
            });

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = self.lock();

            // SAFETY: `nlink` is 0. That is, there is no way to reach to inode,
            // so the `Itable` never tries to obtain an `Rc` referring this `Inode`.
            unsafe {
                A::reacquire_after(guard, move || {
                    ip.itrunc(&tx);
                    ip.deref_inner_mut().typ = InodeType::None;
                    ip.update(&tx);
                    ip.deref_inner_mut().valid = false;
                    drop(ip);
                });
            }
        }
    }
}

impl Inode {
    /// Lock the given inode.
    /// Reads the inode from disk if necessary.
    pub fn lock(&self) -> InodeGuard<'_> {
        let mut guard = self.inner.lock();
        if !guard.valid {
            // TODO: remove kernel_builder()
            let mut bp = kernel_builder().file_system.log.disk.read(
                self.dev,
                // TODO: remove kernel_builder()
                kernel_builder().file_system.superblock().iblock(self.inum),
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
            drop(bp);
            guard.valid = true;
            assert_ne!(guard.typ, InodeType::None, "Inode::lock: no type");
        };
        mem::forget(guard);
        InodeGuard { inode: self }
    }

    pub const fn zero() -> Self {
        Self {
            dev: 0,
            inum: 0,
            inner: Sleeplock::new(
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
    pub fn stat(&self) -> Stat {
        let inner = self.inner.lock();
        Stat {
            dev: self.dev as i32,
            ino: self.inum,
            typ: match inner.typ {
                InodeType::None => 0,
                InodeType::Dir => 1,
                InodeType::File => 2,
                InodeType::Device { .. } => 3,
            },
            nlink: inner.nlink,
            size: inner.size as usize,
        }
    }
}

impl Itable {
    pub const fn zero() -> Self {
        Spinlock::new("ITABLE", ArrayArena::<Inode, NINODE>::new())
    }

    /// Find the inode with number inum on device dev
    /// and return the in-memory copy. Does not lock
    /// the inode and does not read it from disk.
    pub fn get_inode(&self, dev: u32, inum: u32) -> RcInode {
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
    pub fn alloc_inode(&self, dev: u32, typ: InodeType, tx: &FsTransaction<'_>) -> RcInode {
        // TODO: remove kernel_builder()
        for inum in 1..kernel_builder().file_system.superblock().ninodes {
            // TODO: remove kernel_builder()
            let mut bp = kernel_builder()
                .file_system
                .log
                .disk
                // TODO: remove kernel_builder()
                .read(dev, kernel_builder().file_system.superblock().iblock(inum));

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
                tx.write(bp);
                return self.get_inode(dev, inum);
            }
        }
        panic!("[Itable::alloc_inode] no inodes");
    }

    pub fn root(&self) -> RcInode {
        self.get_inode(ROOTDEV, ROOTINO)
    }

    pub fn namei(&self, path: &Path, proc: &CurrentProc<'_>) -> Result<RcInode, ()> {
        Ok(self.namex(path, false, proc)?.0)
    }

    pub fn nameiparent<'s>(
        &self,
        path: &'s Path,
        proc: &CurrentProc<'_>,
    ) -> Result<(RcInode, &'s FileName), ()> {
        let (ip, name_in_path) = self.namex(path, true, proc)?;
        let name_in_path = name_in_path.ok_or(())?;
        Ok((ip, name_in_path))
    }

    fn namex<'s>(
        &self,
        mut path: &'s Path,
        parent: bool,
        proc: &CurrentProc<'_>,
    ) -> Result<(RcInode, Option<&'s FileName>), ()> {
        let mut ptr = if path.is_absolute() {
            self.root()
        } else {
            proc.cwd().clone()
        };

        while let Some((new_path, name)) = path.skipelem() {
            path = new_path;

            let mut ip = ptr.lock();
            if ip.deref_inner().typ != InodeType::Dir {
                return Err(());
            }
            if parent && path.is_empty_string() {
                // Stop one level early.
                drop(ip);
                return Ok((ptr, Some(name)));
            }
            let next = ip.dirlookup(name, self);
            drop(ip);
            ptr = next?.0
        }
        if parent {
            return Err(());
        }
        Ok((ptr, None))
    }
}
