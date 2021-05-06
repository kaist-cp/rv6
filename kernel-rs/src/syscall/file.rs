//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

#![allow(clippy::unit_arg)]

use core::{cell::UnsafeCell, mem};

use arrayvec::ArrayVec;
use bitflags::bitflags;
use cstr_core::CStr;

use crate::{
    arch::addr::UVAddr,
    file::{FileType, InodeFileType, RcFile},
    fs::{FileName, FileSystem, InodeType, Path, Ufs, UfsInodeGuard},
    ok_or,
    page::Page,
    param::{MAXARG, MAXPATH},
    proc::{CurrentProc, KernelCtx},
    some_or,
};

bitflags! {
    struct FcntlFlags: i32 {
        const O_RDONLY = 0;
        const O_WRONLY = 0x1;
        const O_RDWR = 0x2;
        const O_CREATE = 0x200;
        const O_TRUNC = 0x400;
    }
}

impl RcFile {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    fn fdalloc(self, ctx: &mut KernelCtx<'_, '_>) -> Result<i32, Self> {
        let proc_data = ctx.proc_mut().deref_mut_data();
        for (fd, f) in proc_data.open_files.iter_mut().enumerate() {
            if f.is_none() {
                *f = Some(self);
                return Ok(fd as i32);
            }
        }
        Err(self)
    }
}

impl KernelCtx<'_, '_> {
    /// Create an inode with given type.
    /// Returns Ok(created inode, result of given function f) on success, Err(()) on error.
    fn create<F, T>(
        &self,
        path: &Path,
        typ: InodeType,
        tx: &<Ufs as FileSystem>::Tx<'_>,
        f: F,
    ) -> Result<(<Ufs as FileSystem>::Inode, T), ()>
    where
        F: FnOnce(&mut UfsInodeGuard<'_>) -> T,
    {
        let (ptr, name) = self.kernel().fs().itable.nameiparent(path, self)?;
        let mut dp = ptr.lock(self);
        if let Ok((ptr2, _)) = dp.dirlookup(&name, self) {
            drop(dp);
            if typ != InodeType::File {
                return Err(());
            }
            let mut ip = ptr2.lock(self);
            if let InodeType::None | InodeType::Dir = ip.deref_inner().typ {
                return Err(());
            }
            let ret = f(&mut ip);
            drop(ip);
            return Ok((ptr2, ret));
        }
        let ptr2 = self.kernel().fs().itable.alloc_inode(dp.dev, typ, tx, self);
        let mut ip = ptr2.lock(self);
        ip.deref_inner_mut().nlink = 1;
        ip.update(tx, self);

        // Create . and .. entries.
        if typ == InodeType::Dir {
            // for ".."
            dp.deref_inner_mut().nlink += 1;
            dp.update(tx, self);

            // No ip->nlink++ for ".": avoid cyclic ref count.
            // SAFETY: b"." does not contain any NUL characters.
            ip.dirlink(unsafe { FileName::from_bytes(b".") }, ip.inum, tx, self)
                // SAFETY: b".." does not contain any NUL characters.
                .and_then(|_| ip.dirlink(unsafe { FileName::from_bytes(b"..") }, dp.inum, tx, self))
                .expect("create dots");
        }
        dp.dirlink(&name, ip.inum, tx, self)
            .expect("create: dirlink");
        let ret = f(&mut ip);
        drop(ip);
        Ok((ptr2, ret))
    }

    /// Open a file; omode indicate read/write.
    /// Returns Ok(file descriptor) on success, Err(()) on error.
    fn open(&mut self, name: &Path, omode: FcntlFlags) -> Result<usize, ()> {
        let tx = self.kernel().fs().begin_tx();

        let (ip, typ) = if omode.contains(FcntlFlags::O_CREATE) {
            self.create(name, InodeType::File, &tx, |ip| ip.deref_inner().typ)?
        } else {
            let ptr = self.kernel().fs().itable.namei(name, self)?;
            let ip = ptr.lock(self);
            let typ = ip.deref_inner().typ;

            if typ == InodeType::Dir && omode != FcntlFlags::O_RDONLY {
                return Err(());
            }
            drop(ip);
            (ptr, typ)
        };

        let filetype = match typ {
            InodeType::Device { major, .. } => {
                let major = self.kernel().devsw().get(major as usize).ok_or(())?;
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

        let f = self.kernel().ftable.alloc_file(
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
                } => ip.lock(self).itrunc(&tx, self),
                _ => panic!("sys_open : Not reach"),
            };
        }
        let fd = f.fdalloc(self).map_err(|_| ())?;
        Ok(fd as usize)
    }

    /// Create a new directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn mkdir(&self, dirname: &CStr) -> Result<(), ()> {
        let tx = self.kernel().fs().begin_tx();
        self.create(Path::new(dirname), InodeType::Dir, &tx, |_| ())?;
        Ok(())
    }

    /// Create a device file.
    /// Returns Ok(()) on success, Err(()) on error.
    fn mknod(&self, filename: &CStr, major: u16, minor: u16) -> Result<(), ()> {
        let tx = self.kernel().fs().begin_tx();
        self.create(
            Path::new(filename),
            InodeType::Device { major, minor },
            &tx,
            |_| (),
        )?;
        Ok(())
    }

    /// Change the current directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn chdir(&mut self, dirname: &CStr) -> Result<(), ()> {
        // TODO(https://github.com/kaist-cp/rv6/issues/290)
        // The method namei can drop inodes. If namei succeeds, its return
        // value, ptr, will be dropped when this method returns. Deallocation
        // of an inode may cause disk write operations, so we must begin a
        // transaction here.
        let _tx = self.kernel().fs().begin_tx();
        let ptr = self.kernel().fs().itable.namei(Path::new(dirname), self)?;
        let ip = ptr.lock(self);
        if ip.deref_inner().typ != InodeType::Dir {
            return Err(());
        }
        drop(ip);
        let _ = mem::replace(self.proc_mut().cwd_mut(), ptr);
        Ok(())
    }

    /// Create a pipe, put read/write file descriptors in fd0 and fd1.
    /// Returns Ok(()) on success, Err(()) on error.
    fn pipe(&mut self, fdarray: UVAddr) -> Result<(), ()> {
        let (pipereader, pipewriter) = self.kernel().allocate_pipe()?;

        let mut this = scopeguard::guard((self, -1, -1), |(this, fd1, fd2)| {
            if fd1 != -1 {
                this.proc_mut().deref_mut_data().open_files[fd1 as usize] = None;
            }

            if fd2 != -1 {
                this.proc_mut().deref_mut_data().open_files[fd2 as usize] = None;
            }
        });

        this.1 = pipereader.fdalloc(this.0).map_err(|_| ())?;
        this.2 = pipewriter.fdalloc(this.0).map_err(|_| ())?;

        let (this, fd1, fd2) = scopeguard::ScopeGuard::into_inner(this);
        this.proc_mut().memory_mut().copy_out(fdarray, &[fd1, fd2])
    }
}

impl KernelCtx<'_, '_> {
    /// Return a new file descriptor referring to the same file as given fd.
    /// Returns Ok(new file descriptor) on success, Err(()) on error.
    pub fn sys_dup(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let newfile = f.clone();
        let fd = newfile.fdalloc(self).map_err(|_| ())?;
        Ok(fd as usize)
    }

    /// Read n bytes into buf.
    /// Returns Ok(number read) on success, Err(()) on error.
    pub fn sys_read(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let n = self.proc().argint(2)?;
        let p = self.proc().argaddr(1)?;
        // SAFETY: read will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).read(p.into(), n, self) }
    }

    /// Write n bytes from buf to given file descriptor fd.
    /// Returns Ok(n) on success, Err(()) on error.
    pub fn sys_write(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let n = self.proc().argint(2)?;
        let p = self.proc().argaddr(1)?;
        // SAFETY: write will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).write(p.into(), n, self) }
    }

    /// Release open file fd.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_close(&mut self) -> Result<usize, ()> {
        let (fd, _) = self.proc().argfd(0)?;
        self.proc_mut().deref_mut_data().open_files[fd as usize] = None;
        Ok(0)
    }

    /// Place info about an open file into struct stat.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_fstat(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        // user pointer to struct stat
        let st = self.proc().argaddr(1)?;
        // SAFETY: stat will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).stat(st.into(), self) }?;
        Ok(0)
    }

    /// Create the path new as a link to the same inode as old.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_link(&mut self) -> Result<usize, ()> {
        let mut new: [u8; MAXPATH] = [0; MAXPATH];
        let mut old: [u8; MAXPATH] = [0; MAXPATH];
        let old = self.proc_mut().argstr(0, &mut old)?;
        let new = self.proc_mut().argstr(1, &mut new)?;
        let tx = self.kernel().fs().begin_tx();
        self.kernel().fs().link(old, new, &tx, self)?;
        Ok(0)
    }

    /// Remove a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_unlink(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = self.proc_mut().argstr(0, &mut path)?;
        let tx = self.kernel().fs().begin_tx();
        self.kernel().fs().unlink(path, &tx, self)?;
        Ok(0)
    }

    /// Open a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_open(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = self.proc_mut().argstr(0, &mut path)?;
        let path = Path::new(path);
        let omode = self.proc().argint(1)?;
        let omode = FcntlFlags::from_bits_truncate(omode);
        self.open(path, omode)
    }

    /// Create a new directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_mkdir(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = self.proc_mut().argstr(0, &mut path)?;
        self.mkdir(path)?;
        Ok(0)
    }

    /// Create a new directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_mknod(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = self.proc_mut().argstr(0, &mut path)?;
        let major = self.proc().argint(1)? as u16;
        let minor = self.proc().argint(2)? as u16;
        self.mknod(path, major, minor)?;
        Ok(0)
    }

    /// Change the current directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_chdir(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = self.proc_mut().argstr(0, &mut path)?;
        self.chdir(path)?;
        Ok(0)
    }

    /// Load a file and execute it with arguments.
    /// Returns Ok(argc argument to user main) on success, Err(()) on error.
    pub fn sys_exec(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let mut args = ArrayVec::<Page, MAXARG>::new();
        let path = self.proc_mut().argstr(0, &mut path)?;
        let uargv = self.proc().argaddr(1)?;

        let mut success = false;
        for i in 0..MAXARG {
            let uarg = ok_or!(
                self.proc_mut()
                    .fetchaddr((uargv + mem::size_of::<usize>() * i).into()),
                break
            );

            if uarg == 0 {
                success = true;
                break;
            }

            let mut page = some_or!(self.kernel().kmem.alloc(), break);
            if self
                .proc_mut()
                .fetchstr(uarg.into(), &mut page[..])
                .is_err()
            {
                self.kernel().kmem.free(page);
                break;
            }
            args.push(page);
        }

        let ret = if success {
            self.exec(Path::new(path), &args)
        } else {
            Err(())
        };

        for page in args.drain(..) {
            self.kernel().kmem.free(page);
        }

        ret
    }

    /// Create a pipe.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_pipe(&mut self) -> Result<usize, ()> {
        // user pointer to array of two integers
        let fdarray = self.proc().argaddr(0)?.into();
        self.pipe(fdarray)?;
        Ok(0)
    }
}

impl CurrentProc<'_, '_> {
    /// Fetch the nth word-sized system call argument as a file descriptor
    /// and return both the descriptor and the corresponding struct file.
    fn argfd(&self, n: usize) -> Result<(i32, &'_ RcFile), ()> {
        let fd = self.argint(n)?;
        let f = self
            .deref_data()
            .open_files
            .get(fd as usize)
            .ok_or(())?
            .as_ref()
            .ok_or(())?;
        Ok((fd, f))
    }
}
