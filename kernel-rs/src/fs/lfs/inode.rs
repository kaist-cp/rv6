use core::{iter::StepBy, mem, ops::Range};

use static_assertions::const_assert;
use zerocopy::{AsBytes, FromBytes};

use super::{FileName, Lfs, Path, NDIRECT, NINDIRECT, ROOTINO};
use crate::{
    arena::{Arena, ArrayArena},
    bio::BufData,
    fs::{lfs::superblock::IPB, DInodeType, Inode, InodeGuard, InodeType, Itable, RcInode, Tx},
    hal::hal,
    lock::SleepLock,
    param::{NINODE, ROOTDEV},
    proc::KernelCtx,
    util::{memset, strong_pin::StrongPin},
};

/// Directory is a file containing a sequence of Dirent structures.
pub const DIRSIZ: usize = 14;

/// dirent size
pub const DIRENT_SIZE: usize = mem::size_of::<Dirent>();

pub struct InodeInner {
    /// inode has been read from disk?
    pub valid: bool,
    /// type of disk inode
    pub typ: InodeType,
    /// the number of links to this inode
    pub nlink: i16,
    // the size of this inode
    pub size: u32,
    /// direct addresses of disk data
    pub addr_direct: [u32; NDIRECT],
    /// indirect address
    pub addr_indirect: u32,
}

/// On-disk inode structure
///
/// Both the kernel and user programs use this header file.
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[allow(dead_code)]
#[repr(C)]
pub struct Dinode {
    /// File type
    pub typ: DInodeType,

    /// Major device number (T_DEVICE only)
    pub major: u16,

    /// Minor device number (T_DEVICE only)
    pub minor: u16,

    /// Number of links to inode in file system
    pub nlink: i16,

    /// Size of file (bytes)
    pub size: u32,

    /// Direct data block addresses
    pub addr_direct: [u32; NDIRECT],

    /// Indirect data block address
    pub addr_indirect: u32,
}

// TODO: Dirent and following Iter codes are redundant to codes in ufs/inode.rs
// Reduce code using Type generics
#[repr(C)]
#[derive(Default, AsBytes, FromBytes)]
pub struct Dirent {
    pub inum: u16,
    name: [u8; DIRSIZ],
}

impl Dirent {
    fn new(ip: &mut InodeGuard<'_, Lfs>, off: u32, ctx: &KernelCtx<'_, '_>) -> Result<Dirent, ()> {
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

/// DirentIter
///
/// `'id` and `'t` are current lifetime of context that stores information about current thread
/// `'s` is a lifetime for the guard and ctx
struct DirentIter<'id, 's, 't> {
    guard: &'s mut InodeGuard<'t, Lfs>,
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

impl<'t> InodeGuard<'t, Lfs> {
    fn iter_dirents<'id, 's>(&'s mut self, ctx: &'s KernelCtx<'id, 's>) -> DirentIter<'id, 's, 't> {
        let iter = (0..self.deref_inner().size).step_by(DIRENT_SIZE);
        DirentIter {
            guard: self,
            iter,
            ctx,
        }
    }
}

/// InodeGuard
///
/// Handling directories
impl InodeGuard<'_, Lfs> {
    /// Write a new directory entry (name, inum) into the directory dp.
    pub fn dirlink(
        &mut self,
        name: &FileName<DIRSIZ>,
        inum: u32,
        tx: &Tx<'_, Lfs>,
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
        name: &FileName<DIRSIZ>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<Lfs>, u32), ()> {
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

impl InodeGuard<'_, Lfs> {
    /// Copy a modified in-memory inode to disk.
    pub fn update(&self, tx: &Tx<'_, Lfs>, ctx: &KernelCtx<'_, '_>) {
        // 1. Write the inode to segment.
        let mut segment = tx.fs.segment();
        let (mut bp, disk_block_no) = segment.append_updated_inode_block(self.inum, ctx).unwrap();

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
        for (d, s) in (*dip).addr_direct.iter_mut().zip(&inner.addr_direct) {
            *d = *s;
        }
        (*dip).addr_indirect = inner.addr_indirect;

        if tx.fs.segment().is_full() {
            tx.fs.segment().commit(ctx);
        }

        // 2. Write the imap to segment.
        assert!(tx
            .fs
            .imap()
            .set(self.inum, disk_block_no, &mut segment, ctx));
        if tx.fs.segment().is_full() {
            tx.fs.segment().commit(ctx);
        }
        bp.free(ctx);
    }

    /// Inode content
    ///
    /// The content (data) associated with each inode is stored
    /// in blocks on the disk. The first NDIRECT block numbers
    /// are listed in self->addrs[].  The next NINDIRECT blocks are
    /// listed in block self->addr_indirect.
    /// Return the disk block address of the nth block in inode self.
    /// If there is no such block, bmap allocates one.
    #[allow(dead_code)]
    pub fn bmap_or_alloc(&mut self, bn: usize, tx: &Tx<'_, Lfs>, ctx: &KernelCtx<'_, '_>) -> u32 {
        self.bmap_internal(bn, Some(tx), ctx)
    }

    pub fn bmap(&mut self, bn: usize, ctx: &KernelCtx<'_, '_>) -> u32 {
        self.bmap_internal(bn, None, ctx)
    }

    fn bmap_internal(
        &mut self,
        bn: usize,
        tx_opt: Option<&Tx<'_, Lfs>>,
        ctx: &KernelCtx<'_, '_>,
    ) -> u32 {
        let inner = self.deref_inner();

        if bn < NDIRECT {
            let addr = inner.addr_direct[bn];
            if addr == 0 {
                let (_, addr) = tx_opt
                    .expect("bmap: out of range")
                    .balloc(self.dev, self.inum, bn as u32, ctx);
                self.deref_inner_mut().addr_direct[bn] = addr;
            }
            addr
        } else {
            let bn = bn - NDIRECT;
            assert!(bn < NINDIRECT, "bmap: out of range");

            let indirect = inner.addr_indirect;
            if indirect == 0 {
                let (_, indirect) = tx_opt
                    .expect("bmap: out of range")
                    .balloc(self.dev, self.inum, bn as u32, ctx);
                self.deref_inner_mut().addr_indirect = indirect;
            }

            let mut bp = hal().disk().read(self.dev, indirect, ctx);
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "bmap: Buf data unaligned");
            let addr = data[bn];
            if addr == 0 {
                let tx = tx_opt.expect("bmap: out of range");
                let (_, addr) = tx.balloc(self.dev, self.inum, bn as u32, ctx);
                data[bn] = addr;
                // tx.write(bp, ctx);
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

impl const Default for Inode<Lfs> {
    fn default() -> Self {
        Self::new()
    }
}

impl Inode<Lfs> {
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
}

impl Itable<Lfs> {
    pub const fn new_itable() -> Self {
        ArrayArena::<Inode<Lfs>, NINODE>::new("ITABLE")
    }

    /// Find the inode with number inum on device dev
    /// and return the in-memory copy. Does not lock
    /// the inode and does not read it from disk.
    pub fn get_inode(self: StrongPin<'_, Self>, dev: u32, inum: u32) -> RcInode<Lfs> {
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
        tx: &Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> RcInode<Lfs> {
        let mut imap = tx.fs.imap();
        let mut segment = tx.fs.segment();

        // 1. Write the inode.
        let inum = imap.get_empty_inum(ctx).unwrap();
        let (mut bp, disk_block_no) = segment.append_new_inode_block(inum, ctx).unwrap();

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

        // SAFETY: DInode does not have any invariant.
        unsafe { memset(dip, 0u32) };
        match typ {
            InodeType::None => dip.typ = DInodeType::None,
            InodeType::Dir => dip.typ = DInodeType::Dir,
            InodeType::File => dip.typ = DInodeType::File,
            InodeType::Device { major, minor } => {
                dip.typ = DInodeType::Device;
                dip.major = major;
                dip.minor = minor;
            }
        }
        bp.free(ctx);
        if segment.is_full() {
            segment.commit(ctx);
        }

        // 2. Now write the imap.
        assert!(imap.set(inum, disk_block_no, &mut segment, ctx));
        if segment.is_full() {
            segment.commit(ctx);
        }

        self.get_inode(dev, inum)
    }

    pub fn root(self: StrongPin<'_, Self>) -> RcInode<Lfs> {
        self.get_inode(ROOTDEV, ROOTINO)
    }

    pub fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Lfs>,
        proc: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Lfs>, ()> {
        Ok(self.namex(path, false, tx, proc)?.0)
    }

    pub fn nameiparent<'s>(
        self: StrongPin<'_, Self>,
        path: &'s Path,
        tx: &Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<Lfs>, &'s FileName<{ DIRSIZ }>), ()> {
        let (ip, name_in_path) = self.namex(path, true, tx, ctx)?;
        let name_in_path = name_in_path.ok_or(())?;
        Ok((ip, name_in_path))
    }

    fn namex<'s>(
        self: StrongPin<'_, Self>,
        mut path: &'s Path,
        parent: bool,
        tx: &Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(RcInode<Lfs>, Option<&'s FileName<{ DIRSIZ }>>), ()> {
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
