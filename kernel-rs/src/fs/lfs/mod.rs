use core::cell::UnsafeCell;
use core::mem;
use core::ops::Deref;

use super::{
    DInodeType, FcntlFlags, FileName, FileSystem, Inode, InodeGuard, InodeType, Itable, Path,
    RcInode, Stat, Tx,
};
use crate::{
    file::{FileType, InodeFileType},
    hal::hal,
    param::BSIZE,
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

mod cleaner;
mod imap;
mod inode;
mod lfs;
mod segment;
mod superblock;
mod tx;

use imap::Imap;
use inode::{Dinode, Dirent, InodeInner};
pub use lfs::Lfs;
use segment::{SegManager, SegTable};
use superblock::Superblock;
use tx::TxManager;

/// root i-number
const ROOTINO: u32 = 1;

const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE.wrapping_div(mem::size_of::<u32>());
const MAXFILE: usize = NDIRECT.wrapping_add(NINDIRECT);

impl FileSystem for Lfs {
    type Dirent = Dirent;
    type InodeInner = InodeInner;

    fn init(&self, dev: u32, ctx: &KernelCtx<'_, '_>) {
        self.initialize(dev, ctx);
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
        // name-to-inode translation
        self.itable().namei(path, tx, ctx)
    }

    fn link(
        self: StrongPin<'_, Self>,
        inode: RcInode<Self>,
        path: &Path,
        tx: &Tx<'_, Self>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        // Create another name `path` by linking to inode
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
        // remove a file with `path`
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
        // change the current directory
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

    fn tx_begin(&self, ctx: &KernelCtx<'_, '_>) {
        self.tx_manager().begin_op(self, ctx);
    }

    unsafe fn tx_end(&self, tx: &mut Tx<'_, Self>, ctx: &KernelCtx<'_, '_>) {
        // Commits if this was the last outstanding operation.
        self.tx_manager().end_op(self, tx, ctx);
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
        // read inode
        let inner = guard.deref_inner();
        if off > inner.size || off.wrapping_add(n) < off {
            return Ok(0);
        }
        if off + n > inner.size {
            n = inner.size - off;
        }
        let mut tot: u32 = 0;
        while tot < n {
            let bp = guard.readable_data_block(off as usize / BSIZE, &k);
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            let res = f(tot, &bp.data()[begin..end], &mut k);
            bp.free(&k);
            res?;
            tot += m;
            off += m;
        }
        Ok(tot as usize)
    }

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
        tx: &Tx<'_, Lfs>,
        mut k: K,
    ) -> Result<usize, ()> {
        // write the inode
        if off > guard.deref_inner().size {
            return Err(());
        }
        if off.checked_add(n).ok_or(())? as usize > MAXFILE * BSIZE {
            return Err(());
        }
        let mut tot: u32 = 0;
        while tot < n {
            let mut seg = tx.segmanager(&k);
            let mut bp = guard.writable_data_block(off as usize / BSIZE, &mut seg, tx, &k);
            let m = core::cmp::min(n - tot, BSIZE as u32 - off % BSIZE as u32);
            let begin = (off % BSIZE as u32) as usize;
            let end = begin + m as usize;
            let res = f(tot, &mut bp.data_mut()[begin..end], &mut k);
            bp.free(&k);
            if seg.is_full() {
                seg.commit(true, &k);
            }
            seg.free(&k);
            if res.is_err() {
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
        guard.deref_inner_mut().addr_direct = [0; NDIRECT];
        guard.deref_inner_mut().addr_indirect = 0;
        guard.deref_inner_mut().size = 0;
        guard.update(tx, ctx);
    }

    fn inode_lock<'a>(inode: &'a Inode<Self>, ctx: &KernelCtx<'_, '_>) -> InodeGuard<'a, Self> {
        let mut guard = inode.inner.lock(ctx);
        if !guard.valid {
            let fs = ctx.kernel().fs();
            let imap = fs.imap(ctx);
            let bp = hal().disk().read(inode.dev, imap.get(inode.inum, ctx), ctx);
            imap.free(ctx);

            let dip: &Dinode = bp.data().try_into().unwrap();
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

            let mut seg = tx.segmanager(ctx);
            let mut imap = tx.imap(ctx);
            assert!(imap.set(ip.inum, 0, &mut seg, ctx));
            if seg.is_full() {
                seg.commit(true, ctx);
            }
            imap.free(ctx);
            seg.free(ctx);
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
