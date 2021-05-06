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

use cstr_core::CStr;
use spin::Once;

use super::{FcntlFlags, FileName, FileSystem, InodeType, Path, Stat};
use crate::{
    bio::Buf,
    file::{FileType, InodeFileType},
    param::BSIZE,
    proc::{kernel_ctx, KernelCtx},
};

mod inode;
mod log;
mod superblock;

pub use inode::{
    Dinode, Dirent, Inode, InodeGuard, InodeInner, Itable, RcInode, DIRENT_SIZE, DIRSIZ,
};
pub use log::{Log, LogLocked};
pub use superblock::{Superblock, BPB, IPB};

/// root i-number
const ROOTINO: u32 = 1;

const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

pub struct Ufs {
    /// Initializing superblock should run only once because forkret() calls FileSystem::init().
    /// There should be one superblock per disk device, but we run with only one device.
    superblock: Once<Superblock>,
    pub log: Log,
    pub itable: Itable,
}

impl FileSystem for Ufs {
    type Dirent = Dirent;
    type Inode = RcInode;
    type InodeGuard<'s> = InodeGuard<'s, InodeInner>;
    type Tx<'s> = UfsTx<'s>;

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        if !self.superblock.is_completed() {
            let superblock = self
                .superblock
                .call_once(|| Superblock::new(&self.log.disk.read(dev, 1, ctx)));
            self.log
                .init(dev, superblock.logstart as i32, superblock.nlog as i32, ctx);
        }
    }

    fn begin_tx(&self) -> Self::Tx<'_> {
        self.log.begin_op();
        UfsTx { fs: self }
    }

    fn link(
        &self,
        oldname: &CStr,
        newname: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let ptr = self.itable.namei(Path::new(oldname), ctx)?;
        let mut ip = ptr.lock(ctx);
        if ip.deref_inner().typ == InodeType::Dir {
            return Err(());
        }
        ip.deref_inner_mut().nlink += 1;
        ip.update(&tx, ctx);
        drop(ip);

        if let Ok((ptr2, name)) = ctx
            .kernel()
            .fs()
            .itable
            .nameiparent(Path::new(newname), ctx)
        {
            let mut dp = ptr2.lock(ctx);
            if dp.dev != ptr.dev || dp.dirlink(name, ptr.inum, &tx, ctx).is_err() {
            } else {
                return Ok(());
            }
        }

        let mut ip = ptr.lock(ctx);
        ip.deref_inner_mut().nlink -= 1;
        ip.update(&tx, ctx);
        Err(())
    }

    fn unlink(
        &self,
        filename: &CStr,
        tx: &Self::Tx<'_>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        let (ptr, name) = self.itable.nameiparent(Path::new(filename), ctx)?;
        let mut dp = ptr.lock(ctx);

        // Cannot unlink "." or "..".
        if name.as_bytes() == b"." || name.as_bytes() == b".." {
            return Err(());
        }

        let (ptr2, off) = dp.dirlookup(&name, ctx)?;
        let mut ip = ptr2.lock(ctx);
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

    fn open(
        &self,
        name: &Path,
        omode: FcntlFlags,
        tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        let (ip, typ) = if omode.contains(FcntlFlags::O_CREATE) {
            ctx.create(name, InodeType::File, &tx, |ip| ip.deref_inner().typ)?
        } else {
            let ptr = self.itable.namei(name, ctx)?;
            let ip = ptr.lock(ctx);
            let typ = ip.deref_inner().typ;

            if typ == InodeType::Dir && omode != FcntlFlags::O_RDONLY {
                return Err(());
            }
            drop(ip);
            (ptr, typ)
        };

        let filetype = match typ {
            InodeType::Device { major, .. } => {
                let major = ctx.kernel().devsw().get(major as usize).ok_or(())?;
                FileType::Device { ip, major }
            }
            _ => {
                FileType::Inode {
                    inner: InodeFileType {
                        ip,
                        off: UnsafeCell::new(0),
                    },
                }
            }
        };

        let f = ctx.kernel().ftable.alloc_file(
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
                } => ip.lock(ctx).itrunc(&tx, ctx),
                _ => panic!("sys_open : Not reach"),
            };
        }
        let fd = f.fdalloc(ctx).map_err(|_| ())?;
        Ok(fd as usize)
    }

    fn chdir(
        &self,
        dirname: &CStr,
        _tx: &Self::Tx<'_>,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        // TODO(https://github.com/kaist-cp/rv6/issues/290)
        // The method namei can drop inodes. If namei succeeds, its return
        // value, ptr, will be dropped when this method returns. Deallocation
        // of an inode may cause disk write operations, so we must begin a
        // transaction here.
        let ptr = self.itable.namei(Path::new(dirname), ctx)?;
        let ip = ptr.lock(ctx);
        if ip.deref_inner().typ != InodeType::Dir {
            return Err(());
        }
        drop(ip);
        let _ = mem::replace(ctx.proc_mut().cwd_mut(), ptr);
        Ok(())
    }
}

pub struct UfsTx<'s> {
    fs: &'s Ufs,
}

impl Ufs {
    pub const fn zero() -> Self {
        Self {
            superblock: Once::new(),
            log: Log::zero(),
            itable: Itable::new_itable(),
        }
    }

    fn superblock(&self) -> &Superblock {
        self.superblock.get().expect("superblock")
    }
}

impl Drop for UfsTx<'_> {
    fn drop(&mut self) {
        // Called at the end of each FS system call.
        // Commits if this was the last outstanding operation.
        // TODO(https://github.com/kaist-cp/rv6/issues/267): remove kernel_ctx()
        unsafe {
            kernel_ctx(|ctx| self.fs.log.end_op(&ctx));
        }
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
    fn write(&self, b: Buf) {
        self.fs.log.lock().write(b);
    }

    /// Zero a block.
    fn bzero(&self, dev: u32, bno: u32, ctx: &KernelCtx<'_, '_>) {
        let mut buf = unsafe { ctx.kernel().get_bcache() }
            .get_buf(dev, bno)
            .lock();
        buf.deref_inner_mut().data.fill(0);
        buf.deref_inner_mut().valid = true;
        self.write(buf);
    }

    /// Blocks.
    /// Allocate a zeroed disk block.
    fn balloc(&self, dev: u32, ctx: &KernelCtx<'_, '_>) -> u32 {
        for b in num_iter::range_step(0, self.fs.superblock().size, BPB as u32) {
            let mut bp = self
                .fs
                .log
                .disk
                .read(dev, self.fs.superblock().bblock(b), ctx);
            for bi in 0..cmp::min(BPB as u32, self.fs.superblock().size - b) {
                let m = 1 << (bi % 8);
                if bp.deref_inner_mut().data[(bi / 8) as usize] & m == 0 {
                    // Is block free?
                    bp.deref_inner_mut().data[(bi / 8) as usize] |= m; // Mark block in use.
                    self.write(bp);
                    self.bzero(dev, b + bi, ctx);
                    return b + bi;
                }
            }
        }

        panic!("balloc: out of blocks");
    }

    /// Free a disk block.
    fn bfree(&self, dev: u32, b: u32, ctx: &KernelCtx<'_, '_>) {
        let mut bp = self
            .fs
            .log
            .disk
            .read(dev, self.fs.superblock().bblock(b), ctx);
        let bi = b as usize % BPB;
        let m = 1u8 << (bi % 8);
        assert_ne!(
            bp.deref_inner_mut().data[bi / 8] & m,
            0,
            "freeing free block"
        );
        bp.deref_inner_mut().data[bi / 8] &= !m;
        self.write(bp);
    }
}
