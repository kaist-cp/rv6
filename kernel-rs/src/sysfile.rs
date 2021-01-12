//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

#![allow(clippy::unit_arg)]

use crate::{
    fcntl::FcntlFlags,
    file::{FileType, RcFile},
    fs::{Dirent, FileName, FsTransaction, InodeGuard, Path, RcInode, DIRENT_SIZE},
    kernel::{kernel, Kernel},
    ok_or,
    page::Page,
    param::{MAXARG, MAXPATH, NDEV, NOFILE},
    pipe::AllocatedPipe,
    proc::{myproc, Proc},
    riscv::PGSIZE,
    some_or,
    stat::{T_DEVICE, T_DIR, T_FILE},
    syscall::{argaddr, argint, argstr, fetchaddr, fetchstr},
    vm::{KVAddr, UVAddr, VAddr},
};

use core::{cell::UnsafeCell, mem, ptr, slice};

impl RcFile<'static> {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    unsafe fn fdalloc(self) -> Result<i32, Self> {
        let p: *mut Proc = myproc();
        let mut data = &mut *(*p).data.get();
        for fd in 0..NOFILE {
            // user pointer to struct stat
            if data.open_files[fd].is_none() {
                data.open_files[fd] = Some(self);
                return Ok(fd as i32);
            }
        }
        Err(self)
    }
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe fn argfd(n: usize) -> Result<(i32, &'static RcFile<'static>), ()> {
    let fd = argint(n)?;
    if fd < 0 || fd >= NOFILE as i32 {
        return Err(());
    }

    let f = some_or!(
        &(*(*myproc()).data.get()).open_files[fd as usize],
        return Err(())
    );

    Ok((fd, f))
}

unsafe fn create<F, T>(
    path: &Path,
    typ: i16,
    major: u16,
    minor: u16,
    tx: &FsTransaction<'_>,
    f: F,
) -> Result<(RcInode<'static>, T), ()>
where
    F: FnOnce(&mut InodeGuard<'_>) -> T,
{
    let (ptr, name) = path.nameiparent(tx)?;
    let mut dp = ptr.lock(tx);
    if let Ok((ptr2, _)) = dp.dirlookup(&name) {
        drop(dp);
        let mut ip = ptr2.lock(tx);
        if typ == T_FILE && (ip.deref_inner().typ == T_FILE || ip.deref_inner().typ == T_DEVICE) {
            let ret = f(&mut ip);
            mem::drop(ip);
            return Ok((ptr2, ret));
        }
        return Err(());
    }
    let ptr2 = kernel().itable.alloc_inode(dp.dev, typ, tx);
    let mut ip = ptr2.lock(tx);
    ip.deref_inner_mut().major = major;
    ip.deref_inner_mut().minor = minor;
    ip.deref_inner_mut().nlink = 1;
    ip.update();

    // Create . and .. entries.
    if typ == T_DIR {
        // for ".."
        dp.deref_inner_mut().nlink += 1;
        dp.update();

        // No ip->nlink++ for ".": avoid cyclic ref count.
        ip.dirlink(FileName::from_bytes(b"."), ip.inum)
            .and_then(|_| ip.dirlink(FileName::from_bytes(b".."), dp.inum))
            .expect("create dots");
    }
    dp.dirlink(&name, ip.inum).expect("create: dirlink");
    let ret = f(&mut ip);
    mem::drop(ip);
    Ok((ptr2, ret))
}

impl Kernel {
    pub unsafe fn sys_dup(&self) -> usize {
        let (_, f) = ok_or!(argfd(0), return usize::MAX);
        let newfile = f.clone();

        let fd = ok_or!(newfile.fdalloc(), return usize::MAX);
        fd as usize
    }

    pub unsafe fn sys_read(&self) -> usize {
        let (_, f) = ok_or!(argfd(0), return usize::MAX);
        let n = ok_or!(argint(2), return usize::MAX);
        let p = ok_or!(argaddr(1), return usize::MAX);
        ok_or!(f.read(UVAddr::new(p), n), usize::MAX)
    }

    pub unsafe fn sys_write(&self) -> usize {
        let (_, f) = ok_or!(argfd(0), return usize::MAX);
        let n = ok_or!(argint(2), return usize::MAX);
        let p = ok_or!(argaddr(1), return usize::MAX);
        ok_or!(f.write(UVAddr::new(p), n), usize::MAX)
    }

    pub unsafe fn sys_close(&self) -> usize {
        let (fd, _) = ok_or!(argfd(0), return usize::MAX);
        (*(*myproc()).data.get()).open_files[fd as usize] = None;
        0
    }

    pub unsafe fn sys_fstat(&self) -> usize {
        let (_, f) = ok_or!(argfd(0), return usize::MAX);
        // user pointer to struct stat
        let st = ok_or!(argaddr(1), return usize::MAX);
        ok_or!(f.stat(UVAddr::new(st)), return usize::MAX);
        0
    }

    /// Create the path new as a link to the same inode as old.
    pub unsafe fn sys_link(&self) -> usize {
        let mut new: [u8; MAXPATH as usize] = [0; MAXPATH];
        let mut old: [u8; MAXPATH as usize] = [0; MAXPATH];
        let old = ok_or!(argstr(0, &mut old), return usize::MAX);
        let new = ok_or!(argstr(1, &mut new), return usize::MAX);
        let tx = self.file_system.begin_transaction();
        let ptr = ok_or!(Path::new(old).namei(&tx), return usize::MAX);
        let mut ip = ptr.lock(&tx);
        if ip.deref_inner().typ == T_DIR {
            return usize::MAX;
        }
        ip.deref_inner_mut().nlink += 1;
        ip.update();
        drop(ip);

        if let Ok((ptr2, name)) = Path::new(new).nameiparent(&tx) {
            let mut dp = ptr2.lock(&tx);
            if dp.dev != ptr.dev || dp.dirlink(name, ptr.inum).is_err() {
            } else {
                return 0;
            }
        }

        let mut ip = ptr.lock(&tx);
        ip.deref_inner_mut().nlink -= 1;
        ip.update();
        usize::MAX
    }

    pub unsafe fn sys_unlink(&self) -> usize {
        let mut de: Dirent = Default::default();
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        let tx = self.file_system.begin_transaction();
        let (ptr, name) = ok_or!(Path::new(path).nameiparent(&tx), return usize::MAX);
        let mut dp = ptr.lock(&tx);

        // Cannot unlink "." or "..".
        if !(name.as_bytes() == b"." || name.as_bytes() == b"..") {
            // TODO: use other Result related functions
            if let Ok((ptr2, off)) = dp.dirlookup(&name) {
                let mut ip = ptr2.lock(&tx);
                assert!(ip.deref_inner().nlink >= 1, "unlink: nlink < 1");

                if ip.deref_inner().typ != T_DIR || ip.isdirempty() {
                    let bytes_write = dp.write(
                        KVAddr::new(&mut de as *mut Dirent as usize),
                        off,
                        DIRENT_SIZE as u32,
                    );
                    assert_eq!(bytes_write, Ok(DIRENT_SIZE), "unlink: writei");
                    if ip.deref_inner().typ == T_DIR {
                        dp.deref_inner_mut().nlink -= 1;
                        dp.update();
                    }
                    drop(dp);
                    drop(ptr);
                    ip.deref_inner_mut().nlink -= 1;
                    ip.update();
                    return 0;
                }
            }
        }

        usize::MAX
    }

    pub unsafe fn sys_open(&'static self) -> usize {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        let path = Path::new(path);
        let omode = ok_or!(argint(1), return usize::MAX);
        let omode = FcntlFlags::from_bits_truncate(omode);

        let tx = self.file_system.begin_transaction();

        let (ip, (typ, major)) = if omode.contains(FcntlFlags::O_CREATE) {
            ok_or!(
                create(path, T_FILE, 0, 0, &tx, |ip| (
                    ip.deref_inner().typ,
                    ip.deref_inner().major,
                )),
                return usize::MAX
            )
        } else {
            let ptr = ok_or!(path.namei(&tx), return usize::MAX);
            let ip = ptr.lock(&tx);
            let typ = ip.deref_inner().typ;
            let major = ip.deref_inner().major;

            if ip.deref_inner().typ == T_DIR && omode != FcntlFlags::O_RDONLY {
                return usize::MAX;
            }
            mem::drop(ip);
            (ptr, (typ, major))
        };
        if typ == T_DEVICE && (major as usize >= NDEV) {
            return usize::MAX;
        }

        let filetype = if typ == T_DEVICE {
            let major = major;
            FileType::Device { ip, major }
        } else {
            FileType::Inode {
                ip,
                off: UnsafeCell::new(0),
            }
        };
        let f = some_or!(
            self.ftable.alloc_file(
                filetype,
                !omode.intersects(FcntlFlags::O_WRONLY),
                omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR)
            ),
            return usize::MAX
        );

        if omode.contains(FcntlFlags::O_TRUNC) && typ == T_FILE {
            match &f.typ {
                FileType::Device { ip, .. } | FileType::Inode { ip, .. } => ip.lock(&tx).itrunc(),
                _ => panic!("sys_open : Not reach"),
            };
        }
        let fd = ok_or!(f.fdalloc(), return usize::MAX);
        fd as usize
    }

    pub unsafe fn sys_mkdir(&self) -> usize {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let tx = self.file_system.begin_transaction();
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        ok_or!(
            create(Path::new(path), T_DIR, 0, 0, &tx, |_| ()),
            return usize::MAX
        );
        0
    }

    pub unsafe fn sys_mknod(&self) -> usize {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        let major = ok_or!(argint(1), return usize::MAX) as u16;
        let minor = ok_or!(argint(2), return usize::MAX) as u16;
        let tx = self.file_system.begin_transaction();
        let _ip = ok_or!(
            create(Path::new(path), T_DEVICE, major, minor, &tx, |_| ()),
            return usize::MAX
        );
        0
    }

    pub unsafe fn sys_chdir(&self) -> usize {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let p: *mut Proc = myproc();
        let mut data = &mut *(*p).data.get();
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        let tx = self.file_system.begin_transaction();
        let ptr = ok_or!(Path::new(path).namei(&tx), return usize::MAX);
        let ip = ptr.lock(&tx);
        if ip.deref_inner().typ != T_DIR {
            return usize::MAX;
        }
        mem::drop(ip);
        data.cwd = Some(ptr);
        0
    }

    pub unsafe fn sys_exec(&self) -> usize {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let mut argv: [*mut u8; MAXARG] = [ptr::null_mut(); MAXARG];
        let path = ok_or!(argstr(0, &mut path), return usize::MAX);
        let uargv = ok_or!(argaddr(1), return usize::MAX);

        let mut success = false;
        for (i, arg) in argv.iter_mut().enumerate() {
            let mut uarg = 0;
            if fetchaddr(
                UVAddr::new(uargv + mem::size_of::<usize>() * i),
                &mut uarg as *mut usize,
            ) < 0
            {
                break;
            }

            if uarg == 0 {
                *arg = ptr::null_mut();
                success = true;
                break;
            }

            *arg = some_or!(self.alloc(), break).into_usize() as *mut _;

            if fetchstr(UVAddr::new(uarg), slice::from_raw_parts_mut(*arg, PGSIZE)).is_err() {
                break;
            }
        }

        let ret = if success {
            ok_or!(self.exec(Path::new(path), &argv), usize::MAX)
        } else {
            usize::MAX
        };

        for arg in &mut argv[..] {
            if arg.is_null() {
                break;
            }

            self.free(Page::from_usize(*arg as _));
        }

        ret
    }

    pub unsafe fn sys_pipe(&self) -> usize {
        let p: *mut Proc = myproc();
        let mut data = &mut *(*p).data.get();
        // user pointer to array of two integers
        let fdarray = ok_or!(argaddr(0), return usize::MAX);
        let (pipereader, pipewriter) = ok_or!(AllocatedPipe::alloc(), return usize::MAX);

        let mut fd0 = ok_or!(pipereader.fdalloc(), return usize::MAX);
        let mut fd1 = ok_or!(pipewriter.fdalloc(), {
            data.open_files[fd0 as usize] = None;
            return usize::MAX;
        });

        if data
            .pagetable
            .copyout(
                UVAddr::new(fdarray),
                slice::from_raw_parts_mut(&mut fd0 as *mut i32 as *mut u8, mem::size_of::<i32>()),
            )
            .is_err()
            || data
                .pagetable
                .copyout(
                    UVAddr::new(fdarray.wrapping_add(mem::size_of::<i32>())),
                    slice::from_raw_parts_mut(
                        &mut fd1 as *mut i32 as *mut u8,
                        mem::size_of::<i32>(),
                    ),
                )
                .is_err()
        {
            data.open_files[fd0 as usize] = None;
            data.open_files[fd1 as usize] = None;
            return usize::MAX;
        }
        0
    }
}
