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
use core::{cmp, mem};

use pin_project::pin_project;
use spin::Once;

use self::log::Log;
use super::{FcntlFlags, FileName, FileSystem, InodeGuard, InodeType, Itable, Path, RcInode, Stat};
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

pub use inode::{Dinode, Dirent, InodeInner, DIRENT_SIZE, DIRSIZ};
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
    itable: Itable<InodeInner>,
}

impl FileSystem for Ufs {
    type Dirent = Dirent;
    type InodeInner = InodeInner;
    type Tx<'s> = UfsTx<'s>;

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

    fn begin_tx(&self, ctx: &KernelCtx<'_, '_>) -> Self::Tx<'_> {
        self.log().begin_op(ctx);
        UfsTx { fs: self }
    }

    fn root(self: StrongPin<'_, Self>) -> RcInode<Self::InodeInner> {
        self.itable().root()
    }

    fn namei(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<RcInode<Self::InodeInner>, ()> {
        self.itable().namei(path, tx, ctx)
    }

    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self::InodeInner>,
        path: &Path,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let inode = scopeguard::guard(inode, |ptr| ptr.free((tx, ctx)));
        let ip = inode.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        if ip.deref_inner().typ == InodeType::Dir {
            return Err(());
        }
        ip.deref_inner_mut().nlink += 1;
        ip.update(&tx, ctx);
        drop(ip);

        if let Ok((ptr2, name)) = self.itable().nameiparent(path, tx, ctx) {
            let ptr2 = scopeguard::guard(ptr2, |ptr| ptr.free((tx, ctx)));
            let dp = ptr2.lock(ctx);
            let mut dp = scopeguard::guard(dp, |ip| ip.free(ctx));
            if dp.dev == inode.dev && dp.dirlink(name, inode.inum, &tx, ctx).is_ok() {
                return Ok(());
            }
        }

        let ip = inode.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        ip.deref_inner_mut().nlink -= 1;
        ip.update(&tx, ctx);
        Err(())
    }

    fn unlink(
        self: StrongPin<'_, Self>,
        path: &Path,
        tx: &Self::Tx<'_>,
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

        let (ptr2, off) = dp.dirlookup(&name, ctx)?;
        let ptr2 = scopeguard::guard(ptr2, |ptr| ptr.free((tx, ctx)));
        let ip = ptr2.lock(ctx);
        let mut ip = scopeguard::guard(ip, |ip| ip.free(ctx));
        assert!(ip.deref_inner().nlink >= 1, "unlink: nlink < 1");

        if ip.deref_inner().typ == InodeType::Dir && !ip.is_dir_empty(ctx) {
            return Err(());
        }

        dp.write_kernel(&Dirent::default(), off, &tx, ctx)
            .expect("unlink: writei");
        if ip.deref_inner().typ == InodeType::Dir {
            dp.deref_inner_mut().nlink -= 1;
            dp.update(&tx, ctx);
        }
        drop(dp);
        drop(ptr);
        ip.deref_inner_mut().nlink -= 1;
        ip.update(&tx, ctx);
        Ok(())
    }

    fn create<F, T>(
        self: StrongPin<'_, Self>,
        path: &Path,
        typ: InodeType,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
        f: F,
    ) -> Result<(RcInode<Self::InodeInner>, T), ()>
    where
        F: FnOnce(&mut InodeGuard<'_, Self::InodeInner>) -> T,
    {
        let (ptr, name) = self.itable().nameiparent(path, tx, ctx)?;
        let ptr = scopeguard::guard(ptr, |ptr| ptr.free((tx, ctx)));
        let dp = ptr.lock(ctx);
        let mut dp = scopeguard::guard(dp, |ip| ip.free(ctx));
        if let Ok((ptr2, _)) = dp.dirlookup(&name, ctx) {
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
        dp.dirlink(&name, ip.inum, tx, ctx)
            .expect("create: dirlink");
        let ret = f(&mut ip);
        drop(ip);
        Ok((ptr2, ret))
    }

    fn open(
        self: StrongPin<'_, Self>,
        path: &Path,
        omode: FcntlFlags,
        tx: &Self::Tx<'_>,
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
                    ip.itrunc(&tx, ctx);
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
        inode: RcInode<InodeInner>,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let ip = inode.lock(ctx);
        let typ = ip.deref_inner().typ;
        ip.free(ctx);
        if typ != InodeType::Dir {
            inode.free((tx, ctx));
            return Err(());
        }
        mem::replace(ctx.proc_mut().cwd_mut(), inode).free((tx, ctx));
        Ok(())
    }
}

pub struct UfsTx<'s> {
    fs: &'s Ufs,
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
    fn itable<'s>(self: StrongPin<'s, Self>) -> StrongPin<'s, Itable<InodeInner>> {
        unsafe { StrongPin::new_unchecked(&self.as_pin().get_ref().itable) }
    }
}

impl Drop for UfsTx<'_> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("UfsTx must never drop.");
    }
}

impl UfsTx<'_> {
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

    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub fn end(self, ctx: &KernelCtx<'_, '_>) {
        self.fs.log().end_op(ctx);
        mem::forget(self);
    }
}
