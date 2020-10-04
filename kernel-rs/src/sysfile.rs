//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.
use crate::libc;
use crate::{
    exec::exec,
    fcntl::FcntlFlags,
    file::{FileType, Inode, InodeGuard, RcFile},
    fs::{Dirent, FileName, Path},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    ok_or,
    param::{MAXARG, MAXPATH, NDEV, NOFILE},
    pipe::AllocatedPipe,
    proc::{myproc, Proc},
    riscv::PGSIZE,
    some_or,
    stat::{T_DEVICE, T_DIR, T_FILE},
    syscall::{argaddr, argint, argstr, fetchaddr, fetchstr},
};

use core::mem;
use core::ptr;
use core::slice;

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
    let newfile = (*f).dup();

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
    begin_op();
    let ptr = ok_or!(Path::new(old).namei(), {
        end_op();
        return usize::MAX;
    });
    let mut ip = InodeGuard {
        guard: (*ptr).lock(),
        ptr,
    };
    if ip.guard.typ == T_DIR {
        ip.unlockput();
        end_op();
        return usize::MAX;
    }
    ip.guard.nlink += 1;
    ip.update();
    ip.unlock();
    if let Ok((ptr2, name)) = Path::new(new).nameiparent() {
        let mut dp = InodeGuard {
            guard: (*ptr2).lock(),
            ptr: ptr2,
        };
        if (*ptr2).dev != (*ptr).dev || !dp.dirlink(name, (*ptr).inum) {
            dp.unlockput();
        } else {
            dp.unlockput();
            (*ptr).put();
            end_op();
            return 0;
        }
    }
    let mut ip = InodeGuard {
        guard: (*ptr).lock(),
        ptr,
    };
    ip.guard.nlink -= 1;
    ip.update();
    ip.unlockput();
    end_op();
    usize::MAX
}

impl InodeGuard<'_> {
    /// Is the directory dp empty except for "." and ".." ?
    unsafe fn isdirempty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        let mut off = (2usize).wrapping_mul(mem::size_of::<Dirent>()) as i32;
        while (off as u32) < self.guard.size {
            if self
                .read(
                    0,
                    &mut de as *mut Dirent as usize,
                    off as u32,
                    mem::size_of::<Dirent>() as u32,
                )
                .map_or(true, |v| v != mem::size_of::<Dirent>())
            {
                panic!("isdirempty: readi");
            }
            if de.inum as i32 != 0 {
                return false;
            }
            off = (off as usize).wrapping_add(mem::size_of::<Dirent>()) as i32
        }
        true
    }
}

pub unsafe fn sys_unlink() -> usize {
    let mut de: Dirent = Default::default();
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut off: u32 = 0;
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    begin_op();
    let (ptr, name) = ok_or!(Path::new(path).nameiparent(), {
        end_op();
        return usize::MAX;
    });
    let mut dp = InodeGuard {
        guard: (*ptr).lock(),
        ptr,
    };

    // Cannot unlink "." or "..".
    if !(name.as_bytes() == b"." || name.as_bytes() == b"..") {
        let ptr = dp.dirlookup(&name, &mut off);
        // TODO: use other Result related functions
        if ptr.is_ok() {
            let mut ip = InodeGuard {
                guard: (*ptr.unwrap()).lock(),
                ptr: ptr.unwrap(),
            };
            if ip.guard.nlink < 1 {
                panic!("unlink: nlink < 1");
            }
            if ip.guard.typ == T_DIR && !ip.isdirempty() {
                ip.unlockput();
            } else {
                if dp
                    .write(
                        0,
                        &mut de as *mut Dirent as usize,
                        off,
                        mem::size_of::<Dirent>() as u32,
                    )
                    .map_or(true, |v| v != mem::size_of::<Dirent>())
                {
                    panic!("unlink: writei");
                }
                if ip.guard.typ == T_DIR {
                    dp.guard.nlink -= 1;
                    dp.update();
                }
                dp.unlockput();
                ip.guard.nlink -= 1;
                ip.update();
                ip.unlockput();
                end_op();
                return 0;
            }
        }
    }

    dp.unlockput();
    end_op();
    usize::MAX
}

unsafe fn create(path: &Path, typ: i16, major: u16, minor: u16) -> Result<InodeGuard<'static>, ()> {
    let (ptr, name) = ok_or!(path.nameiparent(), return Err(()));
    let mut dp = InodeGuard {
        guard: (*ptr).lock(),
        ptr,
    };
    let ptr2 = dp.dirlookup(&name, ptr::null_mut());
    // TODO: use other Result related functions
    if ptr2.is_ok() {
        dp.unlockput();
        let ip = InodeGuard {
            guard: (*ptr2.unwrap()).lock(),
            ptr: ptr2.unwrap(),
        };
        if typ == T_FILE && (ip.guard.typ == T_FILE || ip.guard.typ == T_DEVICE) {
            return Ok(ip);
        }
        ip.unlockput();
        return Err(());
    }
    let ptr2 = Inode::alloc((*ptr).dev, typ);
    if ptr2.is_null() {
        panic!("create: Inode::alloc");
    }
    let mut ip = InodeGuard {
        guard: (*ptr2).lock(),
        ptr: ptr2,
    };
    ip.guard.major = major;
    ip.guard.minor = minor;
    ip.guard.nlink = 1;
    ip.update();

    // Create . and .. entries.
    if typ == T_DIR {
        // for ".."
        dp.guard.nlink += 1;
        dp.update();

        // No ip->nlink++ for ".": avoid cyclic ref count.
        if !ip.dirlink(FileName::from_bytes(b"."), (*ptr2).inum)
            || !ip.dirlink(FileName::from_bytes(b".."), (*ptr).inum)
        {
            panic!("create dots");
        }
    }
    if !dp.dirlink(&name, (*ptr2).inum) {
        panic!("create: dirlink");
    }
    dp.unlockput();
    Ok(ip)
}

pub unsafe fn sys_open() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let path = Path::new(path);
    let omode = ok_or!(argint(1), return usize::MAX);
    begin_op();
    let omode = FcntlFlags::from_bits_truncate(omode);
    let ip: InodeGuard<'_> = if omode.contains(FcntlFlags::O_CREATE) {
        let ip = create(path, T_FILE, 0, 0);
        if ip.is_err() {
            end_op();
            return usize::MAX;
        }
        ip.unwrap()
    } else {
        let ptr = ok_or!(path.namei(), {
            end_op();
            return usize::MAX;
        });
        let ip = InodeGuard {
            guard: (*ptr).lock(),
            ptr,
        };
        if ip.guard.typ == T_DIR && omode != FcntlFlags::O_RDONLY {
            ip.unlockput();
            end_op();
            return usize::MAX;
        }
        ip
    };
    if ip.guard.typ == T_DEVICE && (ip.guard.major as usize >= NDEV) {
        ip.unlockput();
        end_op();
        return usize::MAX;
    }

    let f = some_or!(
        RcFile::alloc(
            !omode.intersects(FcntlFlags::O_WRONLY),
            omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR)
        ),
        {
            ip.unlockput();
            end_op();
            return usize::MAX;
        }
    );
    let fd = match f.fdalloc() {
        Ok(fd) => fd,
        Err(f) => {
            drop(f);
            ip.unlockput();
            end_op();
            return usize::MAX;
        }
    };
    let f = (*myproc()).open_files[fd as usize].as_mut().unwrap();

    if ip.guard.typ == T_DEVICE {
        (*f).typ = FileType::Device {
            ip: ip.ptr,
            major: ip.guard.major,
        };
    } else {
        (*f).typ = FileType::Inode { ip: ip.ptr, off: 0 };
    }

    ip.unlock();
    end_op();
    fd as usize
}

pub unsafe fn sys_mkdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    begin_op();
    let path = ok_or!(argstr(0, &mut path), {
        end_op();
        return usize::MAX;
    });
    let ip = create(Path::new(path), T_DIR, 0, 0);
    if ip.is_err() {
        end_op();
        return usize::MAX;
    }
    ip.unwrap().unlockput();
    end_op();
    0
}

pub unsafe fn sys_mknod() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    begin_op();
    let _end_op = scopeguard::guard((), |_| {
        end_op();
    });
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let major = ok_or!(argint(1), return usize::MAX) as u16;
    let minor = ok_or!(argint(2), return usize::MAX) as u16;
    let ip = create(Path::new(path), T_DEVICE, major, minor);
    if ip.is_err() {
        return usize::MAX;
    }
    ip.unwrap().unlockput();
    0
}

pub unsafe fn sys_chdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut p: *mut Proc = myproc();
    begin_op();
    let path = ok_or!(argstr(0, &mut path), {
        end_op();
        return usize::MAX;
    });
    let ptr = ok_or!(Path::new(path).namei(), {
        end_op();
        return usize::MAX;
    });
    let ip = InodeGuard {
        guard: (*ptr).lock(),
        ptr,
    };
    if ip.guard.typ != T_DIR {
        ip.unlockput();
        end_op();
        return usize::MAX;
    }
    ip.unlock();
    (*(*p).cwd).put();
    end_op();
    (*p).cwd = ptr;
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

        *arg = kalloc() as *mut u8;
        if arg.is_null() {
            panic!("sys_exec kalloc");
        }

        if fetchstr(uarg, slice::from_raw_parts_mut(*arg, PGSIZE)).is_err() {
            break;
        }
    }

    let ret = if success {
        ok_or!(exec(Path::new(path), argv.as_mut_ptr()), usize::MAX)
    } else {
        usize::MAX
    };

    for arg in &mut argv[..] {
        if arg.is_null() {
            break;
        }

        kfree(*arg as *mut libc::CVoid);
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
            fdarray,
            &mut fd0 as *mut i32 as *mut u8,
            mem::size_of::<i32>(),
        )
        .is_err()
        || (*p)
            .pagetable
            .assume_init_mut()
            .copyout(
                fdarray.wrapping_add(mem::size_of::<i32>()),
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
