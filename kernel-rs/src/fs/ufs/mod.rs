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
//!
//! On-disk file system format used for both kernel and user programs are also included here.

use core::cell::UnsafeCell;
use core::ops::Deref;
use core::{cmp, mem};

use pin_project::pin_project;
use spin::Once;

use self::log::Log;
use super::{
    FcntlFlags, FileName, FileSystem, Inode, InodeGuard, InodeType, Itable, Path, RcInode, Stat, Tx,
};
use crate::util::strong_pin::StrongPin;
use crate::{
    bio::Buf,
    file::{FileType, InodeFileType},
    hal::hal,
    lock::SleepableLock,
    param::BSIZE,
    proc::KernelCtx,
};

mod inode;
mod log;
mod superblock;

pub use inode::{DInodeType, Dinode, Dirent, InodeInner, DIRENT_SIZE, DIRSIZ};
pub use superblock::{Superblock, BPB, IPB};

/// root i-number
const ROOTINO: u32 = 1;

const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

#[pin_project]
pub struct Ufs {
    /// Initializing superblock should run only once because forkret() calls FileSystem::init().
    /// There should be one superblock per disk device, but we run with only one device.
    superblock: Once<Superblock>,
    log: Once<SleepableLock<Log>>,
    #[pin]
    itable: Itable<Self>,
}

impl Ufs {
    pub const fn new() -> Self {
        Self {
            superblock: Once::new(),
            log: Once::new(),
            itable: Itable::new_itable(),
        }
    }

    fn log(&self) -> &SleepableLock<Log> {
        self.log.get().expect("log")
    }

    fn superblock(&self) -> &Superblock {
        self.superblock.get().expect("superblock")
    }

    #[allow(clippy::needless_lifetimes)]
    fn itable<'s>(self: StrongPin<'s, Self>) -> StrongPin<'s, Itable<Self>> {
        unsafe { StrongPin::new_unchecked(&self.as_pin().get_ref().itable) }
    }
}

impl Tx<'_, Ufs> {
    /// Caller has modified b->data and is done with the buffer.
    /// Record the block number and pin in the cache by increasing refcnt.
    /// commit()/write_log() will do the disk write.
    ///
    /// write() replaces write(); a typical use is:
    ///   bp = kernel.fs().disk.read(...)
    ///   modify bp->data[]
    ///   write(bp)
    fn write(&self, b: Buf, ctx: &KernelCtx<'_, '_>) {
        self.fs.log().lock().write(b, ctx);
    }

    /// Zero a block.
    fn bzero(&self, dev: u32, bno: u32, ctx: &KernelCtx<'_, '_>) {
        let mut buf = ctx.kernel().bcache().get_buf(dev, bno).lock(ctx);
        buf.deref_inner_mut().data.fill(0);
        buf.deref_inner_mut().valid = true;
        self.write(buf, ctx);
    }

    /// Blocks.
    /// Allocate a zeroed disk block.
    fn balloc(&self, dev: u32, ctx: &KernelCtx<'_, '_>) -> u32 {
        for b in num_iter::range_step(0, self.fs.superblock().size, BPB as u32) {
            let mut bp = hal().disk().read(dev, self.fs.superblock().bblock(b), ctx);
            for bi in 0..cmp::min(BPB as u32, self.fs.superblock().size - b) {
                let m = 1 << (bi % 8);
                if bp.deref_inner_mut().data[(bi / 8) as usize] & m == 0 {
                    // Is block free?
                    bp.deref_inner_mut().data[(bi / 8) as usize] |= m; // Mark block in use.
                    self.write(bp, ctx);
                    self.bzero(dev, b + bi, ctx);
                    return b + bi;
                }
            }
            bp.free(ctx);
        }

        panic!("balloc: out of blocks");
    }

    /// Free a disk block.
    fn bfree(&self, dev: u32, b: u32, ctx: &KernelCtx<'_, '_>) {
        let mut bp = hal().disk().read(dev, self.fs.superblock().bblock(b), ctx);
        let bi = b as usize % BPB;
        let m = 1u8 << (bi % 8);
        assert_ne!(
            bp.deref_inner_mut().data[bi / 8] & m,
            0,
            "freeing free block"
        );
        bp.deref_inner_mut().data[bi / 8] &= !m;
        self.write(bp, ctx);
    }
}

impl FileSystem for Ufs {
    type Dirent = Dirent;
    type InodeInner = InodeInner;

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        if !self.superblock.is_completed() {
            let buf = hal().disk().read(dev, 1, ctx);
            let superblock = self.superblock.call_once(|| Superblock::new(&buf));
            buf.free(ctx);
            let _ = self.log.call_once(|| {
                SleepableLock::new(
                    "LOG",
                    Log::new(dev, superblock.logstart as i32, superblock.nlog as i32, ctx),
                )
            });
        }
    }

    fn root(self: StrongPin<'_, Self>) -> RcInode<Self> {
        self.itable().root()
    }

    fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Self>, ()> {
        self.itable().namei(path, tx, ctx)
    }

    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let inode = scopeguard::guard(inode, |ptr| ptr.free((tx, ctx)));
        let ip = inode.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        if ip.deref_inner().typ == InodeType::Dir {
            return Err(());
        }
        ip.deref_inner_mut().nlink += 1;
        ip.update(tx, ctx);
        drop(ip);

        if let Ok((ptr2, name)) = self.itable().nameiparent(path, tx, ctx) {
            let ptr2 = scopeguard::guard(ptr2, |ptr| ptr.free((tx, ctx)));
            let dp = ptr2.lock(ctx);
            let mut dp = scopeguard::guard(dp, |ip| ip.free(ctx));
            if dp.dev == inode.dev && dp.dirlink(name, inode.inum, tx, ctx).is_ok() {
                return Ok(());
            }
        }

        let ip = inode.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        ip.deref_inner_mut().nlink -= 1;
        ip.update(tx, ctx);
        Err(())
    }

    fn unlink(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let (ptr, name) = self.itable().nameiparent(path, tx, ctx)?;
        let ptr = scopeguard::guard(ptr, |ptr| ptr.free((tx, ctx)));
        let dp = ptr.lock(ctx);
        let mut dp = scopeguard::guard(dp, |ip| ip.free(ctx));

        // Cannot unlink "." or "..".
        if name.as_bytes() == b"." || name.as_bytes() == b".." {
            return Err(());
        }

        let (ptr2, off) = dp.dirlookup(name, ctx)?;
        let ptr2 = scopeguard::guard(ptr2, |ptr| ptr.free((tx, ctx)));
        let ip = ptr2.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        assert!(ip.deref_inner().nlink >= 1, "unlink: nlink < 1");

        if ip.deref_inner().typ == InodeType::Dir && !ip.is_dir_empty(ctx) {
            return Err(());
        }

        dp.write_kernel(&Dirent::default(), off, tx, ctx)
            .expect("unlink: writei");
        if ip.deref_inner().typ == InodeType::Dir {
            dp.deref_inner_mut().nlink -= 1;
            dp.update(tx, ctx);
        }
        drop(dp);
        drop(ptr);
        ip.deref_inner_mut().nlink -= 1;
        ip.update(tx, ctx);
        Ok(())
    }

    fn create<F, T>(
        self: StrongPin<'_, Self>,
        path: &Path,
        typ: InodeType,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
        f: F,
    ) -> Result<(RcInode<Self>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self>) -> T,
    {
        let (ptr, name) = self.itable().nameiparent(path, tx, ctx)?;
        let ptr = scopeguard::guard(ptr, |ptr| ptr.free((tx, ctx)));
        let dp = ptr.lock(ctx);
        let mut dp = scopeguard::guard(dp, |ip| ip.free(ctx));
        if let Ok((ptr2, _)) = dp.dirlookup(name, ctx) {
            let ptr2 = scopeguard::guard(ptr2, |ptr| ptr.free((tx, ctx)));
            drop(dp);
            if typ != InodeType::File {
                return Err(());
            }
            let ip = ptr2.lock(ctx);
            let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
            if let InodeType::None | InodeType::Dir = ip.deref_inner().typ {
                return Err(());
            }
            let ret = f(&mut ip);
            drop(ip);
            return Ok((scopeguard::ScopeGuard::into_inner(ptr2), ret));
        }
        let ptr2 = self.itable().alloc_inode(dp.dev, typ, tx, ctx);
        let ip = ptr2.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        ip.deref_inner_mut().nlink = 1;
        ip.update(tx, ctx);

        // Create . and .. entries.
        if typ == InodeType::Dir {
            // for ".."
            dp.deref_inner_mut().nlink += 1;
            dp.update(tx, ctx);

            let inum = ip.inum;
            // No ip->nlink++ for ".": avoid cyclic ref count.
            // SAFETY: b"." does not contain any NUL characters.
            ip.dirlink(unsafe { FileName::from_bytes(b".") }, inum, tx, ctx)
                // SAFETY: b".." does not contain any NUL characters.
                .and_then(|_| ip.dirlink(unsafe { FileName::from_bytes(b"..") }, dp.inum, tx, ctx))
                .expect("create dots");
        }
        dp.dirlink(name, ip.inum, tx, ctx).expect("create: dirlink");
        let ret = f(&mut ip);
        drop(ip);
        Ok((ptr2, ret))
    }

    fn open(
        self: StrongPin<'_, Self>,
        path: &Path,
        omode: FcntlFlags,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        let (ip, typ) = if omode.contains(FcntlFlags::O_CREATE) {
            self.create(path, InodeType::File, tx, ctx, |ip| ip.deref_inner().typ)?
        } else {
            let ptr = self.itable().namei(path, tx, ctx)?;
            let ptr = scopeguard::guard(ptr, |ptr| ptr.free((tx, ctx)));
            let ip = ptr.lock(ctx);
            let ip = scopeguard::guard(ip, |ip| ip.free(ctx));
            let typ = ip.deref_inner().typ;

            if typ == InodeType::Dir && omode != FcntlFlags::O_RDONLY {
                return Err(());
            }
            drop(ip);
            (scopeguard::ScopeGuard::into_inner(ptr), typ)
        };

        let filetype = match typ {
            InodeType::Device { major, .. } => FileType::Device { ip, major },
            _ => {
                FileType::Inode {
                    inner: InodeFileType {
                        ip,
                        off: UnsafeCell::new(0),
                    },
                }
            }
        };

        let f = ctx.kernel().ftable().alloc_file(
            filetype,
            !omode.intersects(FcntlFlags::O_WRONLY),
            omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR),
        )?;

        if omode.contains(FcntlFlags::O_TRUNC) && typ == InodeType::File {
            match &f.typ {
                // It is safe to call itrunc because ip.lock() is held
                FileType::Device { ip, .. }
                | FileType::Inode {
                    inner: InodeFileType { ip, .. },
                } => {
                    let mut ip = ip.lock(ctx);
                    ip.trunc(tx, ctx);
                    ip.free(ctx);
                }
                _ => panic!("sys_open : Not reach"),
            };
        }
        let fd = f.fdalloc(ctx)?;
        Ok(fd as usize)
    }

    fn chdir(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self>,
        tx: &Tx<'_, Self>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let ip = inode.lock(ctx);
        let typ = ip.deref_inner().typ;
        ip.free(ctx);
        if typ != InodeType::Dir {
            inode.free((tx, ctx));
            return Err(());
        }
        let mut guard = ctx.proc().lock();
        let inode = unsafe { mem::replace(guard.deref_mut_info().cwd.assume_init_mut(), inode) };
        drop(guard);
        inode.free((tx, ctx));
        Ok(())
    }

    fn tx_begin(&self, ctx: &KernelCtx<'_, '_>) {
        self.log().begin_op(ctx);
    }

    unsafe fn tx_end(&self, ctx: &KernelCtx<'_, '_>) {
        // Commits if this was the last outstanding operation.
        self.log().end_op(ctx);
    }

    #[inline]
    fn inode_read<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &[u8], &mut K) -> Result<(), ()>,
    >(
        guard: &mut InodeGuard<'_, Self>,
        mut off: u32,
        mut n: u32,
        mut f: F,
        mut k: K,
    ) -> Result<usize, ()> {
        let inner = guard.deref_inner();
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
                .read(guard.dev, guard.bmap(off as usize / BSIZE, &k), &k);
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

    #[inline]
    fn inode_write<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &mut [u8], &mut K) -> Result<(), ()>,
    >(
        guard: &mut InodeGuard<'_, Self>,
        mut off: u32,
        n: u32,
        mut f: F,
        tx: &Tx<'_, Self>,
        mut k: K,
    ) -> Result<usize, ()> {
        if off > guard.deref_inner().size {
            return Err(());
        }
        if off.checked_add(n).ok_or(())? as usize > MAXFILE * BSIZE {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut bp = hal().disk().read(
                guard.dev,
                guard.bmap_or_alloc(off as usize / BSIZE, tx, &k),
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

        if off > guard.deref_inner().size {
            guard.deref_inner_mut().size = off;
        }

        // Write the i-node back to disk even if the size didn't change
        // because the loop above might have called bmap() and added a new
        // block to self->addrs[].
        guard.update(tx, &k);
        Ok(tot as usize)
    }

    fn inode_trunc(guard: &mut InodeGuard<'_, Self>, tx: &Tx<'_, Self>, ctx: &KernelCtx<'_, '_>) {
        let dev = guard.dev;
        for addr in &mut guard.deref_inner_mut().addr_direct {
            if *addr != 0 {
                tx.bfree(dev, *addr, ctx);
                *addr = 0;
            }
        }

        if guard.deref_inner().addr_indirect != 0 {
            let mut bp = hal()
                .disk()
                .read(dev, guard.deref_inner().addr_indirect, ctx);
            // SAFETY: u32 does not have internal structure.
            let (prefix, data, _) = unsafe { bp.deref_inner_mut().data.align_to_mut::<u32>() };
            debug_assert_eq!(prefix.len(), 0, "itrunc: Buf data unaligned");
            for a in data {
                if *a != 0 {
                    tx.bfree(dev, *a, ctx);
                }
            }
            bp.free(ctx);
            tx.bfree(dev, guard.deref_inner().addr_indirect, ctx);
            guard.deref_inner_mut().addr_indirect = 0
        }

        guard.deref_inner_mut().size = 0;
        guard.update(tx, ctx);
    }

    fn inode_lock<'a>(inode: &'a Inode<Self>, ctx: &KernelCtx<'_, '_>) -> InodeGuard<'a, Self> {
        let mut guard = inode.inner.lock(ctx);
        if !guard.valid {
            let mut bp = hal().disk().read(
                inode.dev,
                ctx.kernel().fs().superblock().iblock(inode.inum),
                ctx,
            );

            // SAFETY: dip is inside bp.data.
            let dip = unsafe {
                (bp.deref_inner_mut().data.as_mut_ptr() as *mut Dinode)
                    .add(inode.inum as usize % IPB)
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
            for (d, s) in guard.addr_direct.iter_mut().zip(&dip.addr_direct) {
                *d = *s;
            }
            guard.addr_indirect = dip.addr_indirect;
            bp.free(ctx);
            guard.valid = true;
            assert_ne!(guard.typ, InodeType::None, "Inode::lock: no type");
        };
        mem::forget(guard);
        InodeGuard { inode }
    }

    fn inode_finalize<'a, 'id: 'a>(
        inode: &mut Inode<Self>,
        tx: &'a Tx<'a, Self>,
        ctx: &'a KernelCtx<'id, 'a>,
    ) {
        if inode.inner.get_mut().valid && inode.inner.get_mut().nlink == 0 {
            // inode has no links and no other references: truncate and free.

            // self->ref == 1 means no other process can have self locked,
            // so this acquiresleep() won't block (or deadlock).
            let mut ip = inode.lock(ctx);

            ip.trunc(tx, ctx);
            ip.deref_inner_mut().typ = InodeType::None;
            ip.update(tx, ctx);
            ip.deref_inner_mut().valid = false;

            ip.free(ctx);
        }
    }

    fn inode_stat(inode: &Inode<Self>, ctx: &KernelCtx<'_, '_>) -> Stat {
        let inner = inode.inner.lock(ctx);
        let st = Stat {
            dev: inode.dev as i32,
            ino: inode.inum,
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
