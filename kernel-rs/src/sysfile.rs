//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

#![allow(clippy::unit_arg)]

use crate::{
    exec::exec,
    fcntl::FcntlFlags,
    file::{FileType, RcFile},
    fs::{fs, Dirent, FileName, Inode, InodeGuard, Path, RcInode, DIRENT_SIZE},
    kernel::kernel,
    ok_or,
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

impl RcFile {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    unsafe fn fdalloc(self) -> Result<i32, Self> {
        let mut p: *mut Proc = myproc();
        for fd in 0..NOFILE {
            // user pointer to struct stat
            if (*p).open_files[fd].is_none() {
                (*p).open_files[fd] = Some(self);
                return Ok(fd as i32);
            }
        }
        Err(self)
    }
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe fn argfd(n: usize) -> Result<(i32, *mut RcFile), ()> {
    let fd = argint(n)?;
    if fd < 0 || fd >= NOFILE as i32 {
        return Err(());
    }

    let f = some_or!(&mut (*myproc()).open_files[fd as usize], return Err(()));

    Ok((fd, f))
}

pub unsafe fn sys_dup() -> usize {
    let (_, f) = ok_or!(argfd(0), return usize::MAX);
    let newfile = (*f).clone();

    let fd = ok_or!(newfile.fdalloc(), return usize::MAX);
    fd as usize
}

pub unsafe fn sys_read() -> usize {
    let (_, f) = ok_or!(argfd(0), return usize::MAX);
    let n = ok_or!(argint(2), return usize::MAX);
    let p = ok_or!(argaddr(1), return usize::MAX);
    ok_or!((*f).read(p, n), usize::MAX)
}

pub unsafe fn sys_write() -> usize {
    let (_, f) = ok_or!(argfd(0), return usize::MAX);
    let n = ok_or!(argint(2), return usize::MAX);
    let p = ok_or!(argaddr(1), return usize::MAX);
    ok_or!((*f).write(p, n), usize::MAX)
}

pub unsafe fn sys_close() -> usize {
    let (fd, _) = ok_or!(argfd(0), return usize::MAX);
    (*myproc()).open_files[fd as usize] = None;
    0
}

pub unsafe fn sys_fstat() -> usize {
    let (_, f) = ok_or!(argfd(0), return usize::MAX);
    // user pointer to struct stat
    let st = ok_or!(argaddr(1), return usize::MAX);
    ok_or!((*f).stat(st), return usize::MAX);
    0
}

/// Create the path new as a link to the same inode as old.
pub unsafe fn sys_link() -> usize {
    let mut new: [u8; MAXPATH as usize] = [0; MAXPATH];
    let mut old: [u8; MAXPATH as usize] = [0; MAXPATH];
    let old = ok_or!(argstr(0, &mut old), return usize::MAX);
    let new = ok_or!(argstr(1, &mut new), return usize::MAX);
    let _tx = fs().begin_transaction();
    let ptr = ok_or!(Path::new(old).namei(), return usize::MAX);
    let mut ip = ptr.lock();
    if ip.deref_inner().typ == T_DIR {
        return usize::MAX;
    }
    ip.deref_inner_mut().nlink += 1;
    ip.update();
    drop(ip);

    if let Ok((ptr2, name)) = Path::new(new).nameiparent() {
        let mut dp = ptr2.lock();
        if dp.dev != ptr.dev || dp.dirlink(name, ptr.inum).is_err() {
        } else {
            return 0;
        }
    }

    let mut ip = ptr.lock();
    ip.deref_inner_mut().nlink -= 1;
    ip.update();
    usize::MAX
}

pub unsafe fn sys_unlink() -> usize {
    let mut de: Dirent = Default::default();
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let _tx = fs().begin_transaction();
    let (ptr, name) = ok_or!(Path::new(path).nameiparent(), return usize::MAX);
    let mut dp = ptr.lock();

    // Cannot unlink "." or "..".
    if !(name.as_bytes() == b"." || name.as_bytes() == b"..") {
        // TODO: use other Result related functions
        if let Ok((ptr2, off)) = dp.dirlookup(&name) {
            let mut ip = ptr2.lock();
            if ip.deref_inner().nlink < 1 {
                panic!("unlink: nlink < 1");
            }
            if ip.deref_inner().typ != T_DIR || ip.isdirempty() {
                let bytes_write = dp.write(
                    KVAddr::wrap(&mut de as *mut Dirent as usize),
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

unsafe fn create<F, T>(
    path: &Path,
    typ: i16,
    major: u16,
    minor: u16,
    f: F,
) -> Result<(RcInode, T), ()>
where
    F: FnOnce(&mut InodeGuard<'_>) -> T,
{
    let (ptr, name) = path.nameiparent()?;
    let mut dp = ptr.lock();
    if let Ok((ptr2, _)) = dp.dirlookup(&name) {
        drop(dp);
        let mut ip = ptr2.lock();
        if typ == T_FILE && (ip.deref_inner().typ == T_FILE || ip.deref_inner().typ == T_DEVICE) {
            let ret = f(&mut ip);
            mem::drop(ip);
            return Ok((ptr2, ret));
        }
        return Err(());
    }
    let ptr2 = Inode::alloc(dp.dev, typ);
    let mut ip = ptr2.lock();
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

pub unsafe fn sys_open() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let path = Path::new(path);
    let omode = ok_or!(argint(1), return usize::MAX);
    let omode = FcntlFlags::from_bits_truncate(omode);

    let _tx = fs().begin_transaction();

    let (ip, (typ, major)) = if omode.contains(FcntlFlags::O_CREATE) {
        ok_or!(
            create(path, T_FILE, 0, 0, |ip| (
                ip.deref_inner().typ,
                ip.deref_inner().major
            )),
            return usize::MAX
        )
    } else {
        let ptr = ok_or!(path.namei(), return usize::MAX);
        let ip = ptr.lock();
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

    let typ = if typ == T_DEVICE {
        let major = major;
        FileType::Device { ip, major }
    } else {
        FileType::Inode {
            ip,
            off: UnsafeCell::new(0),
        }
    };
    let f = some_or!(
        RcFile::alloc(
            typ,
            !omode.intersects(FcntlFlags::O_WRONLY),
            omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR)
        ),
        return usize::MAX
    );
    let fd = ok_or!(f.fdalloc(), return usize::MAX);
    fd as usize
}

pub unsafe fn sys_mkdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let _tx = fs().begin_transaction();
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    ok_or!(
        create(Path::new(path), T_DIR, 0, 0, |_| ()),
        return usize::MAX
    );
    0
}

pub unsafe fn sys_mknod() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let major = ok_or!(argint(1), return usize::MAX) as u16;
    let minor = ok_or!(argint(2), return usize::MAX) as u16;
    let _tx = fs().begin_transaction();
    let _ip = ok_or!(
        create(Path::new(path), T_DEVICE, major, minor, |_| ()),
        return usize::MAX
    );
    0
}

pub unsafe fn sys_chdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut p: *mut Proc = myproc();
    let _tx = fs().begin_transaction();
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let ptr = ok_or!(Path::new(path).namei(), return usize::MAX);
    let ip = ptr.lock();
    if ip.deref_inner().typ != T_DIR {
        return usize::MAX;
    }
    mem::drop(ip);
    (*p).cwd = Some(ptr);
    0
}

pub unsafe fn sys_exec() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut argv: [*mut u8; MAXARG] = [ptr::null_mut(); MAXARG];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let uargv = ok_or!(argaddr(1), return usize::MAX);

    let mut success = false;
    for (i, arg) in argv.iter_mut().enumerate() {
        let mut uarg = 0;
        if fetchaddr(uargv + mem::size_of::<usize>() * i, &mut uarg as *mut usize) < 0 {
            break;
        }

        if uarg == 0 {
            *arg = ptr::null_mut();
            success = true;
            break;
        }

        *arg = kernel().alloc();
        if arg.is_null() {
            panic!("sys_exec kalloc");
        }

        if fetchstr(uarg, slice::from_raw_parts_mut(*arg, PGSIZE)).is_err() {
            break;
        }
    }

    let ret = if success {
        ok_or!(exec(Path::new(path), &argv), usize::MAX)
    } else {
        usize::MAX
    };

    for arg in &mut argv[..] {
        if arg.is_null() {
            break;
        }

        kernel().free(*arg);
    }

    ret
}

pub unsafe fn sys_pipe() -> usize {
    let mut p: *mut Proc = myproc();
    // user pointer to array of two integers
    let fdarray = ok_or!(argaddr(0), return usize::MAX);
    let (pipereader, pipewriter) = ok_or!(AllocatedPipe::alloc(), return usize::MAX);

    let mut fd0 = ok_or!(pipereader.fdalloc(), return usize::MAX);
    let mut fd1 = ok_or!(pipewriter.fdalloc(), {
        (*p).open_files[fd0 as usize] = None;
        return usize::MAX;
    });

    if (*p)
        .pagetable
        .assume_init_mut()
        .copyout(
            UVAddr::wrap(fdarray),
            &mut fd0 as *mut i32 as *mut u8,
            mem::size_of::<i32>(),
        )
        .is_err()
        || (*p)
            .pagetable
            .assume_init_mut()
            .copyout(
                UVAddr::wrap(fdarray.wrapping_add(mem::size_of::<i32>())),
                &mut fd1 as *mut i32 as *mut u8,
                mem::size_of::<i32>(),
            )
            .is_err()
    {
        (*p).open_files[fd0 as usize] = None;
        (*p).open_files[fd1 as usize] = None;
        return usize::MAX;
    }
    0
}
