//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.
use crate::libc;
use crate::{
    exec::exec,
    fcntl::FcntlFlags,
    file::{FileType, Inode, InodeGuard, RcFile},
    fs::{Dirent, FileName, Path, DIRENT_SIZE},
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
    let mut ip = (*ptr).lock();
    if ip.typ == T_DIR {
        ip.unlockput();
        end_op();
        return usize::MAX;
    }
    ip.nlink += 1;
    ip.update();
    drop(ip);
    if let Ok((ptr2, name)) = Path::new(new).nameiparent() {
        let mut dp = (*ptr2).lock();
        if (*ptr2).dev != (*ptr).dev || dp.dirlink(name, (*ptr).inum).is_err() {
            dp.unlockput();
        } else {
            dp.unlockput();
            (*ptr).put();
            end_op();
            return 0;
        }
    }
    let mut ip = (*ptr).lock();
    ip.nlink -= 1;
    ip.update();
    ip.unlockput();
    end_op();
    usize::MAX
}

impl InodeGuard<'_> {
    /// Is the directory dp empty except for "." and ".." ?
    unsafe fn isdirempty(&mut self) -> bool {
        let mut de: Dirent = Default::default();
        for off in (2 * DIRENT_SIZE as u32..self.size).step_by(DIRENT_SIZE) {
            let bytes_read = self.read(
                0,
                &mut de as *mut Dirent as usize,
                off as u32,
                DIRENT_SIZE as u32,
            );
            assert_eq!(bytes_read, Ok(DIRENT_SIZE), "isdirempty: readi");
            if de.inum != 0 {
                return false;
            }
        }
        true
    }
}

pub unsafe fn sys_unlink() -> usize {
    let mut de: Dirent = Default::default();
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    begin_op();
    let (ptr, name) = ok_or!(Path::new(path).nameiparent(), {
        end_op();
        return usize::MAX;
    });
    let mut dp = (*ptr).lock();

    // Cannot unlink "." or "..".
    if !(name.as_bytes() == b"." || name.as_bytes() == b"..") {
        // TODO: use other Result related functions
        if let Ok((ptr2, off)) = dp.dirlookup(&name) {
            let mut ip = (*ptr2).lock();
            if ip.nlink < 1 {
                panic!("unlink: nlink < 1");
            }
            if ip.typ == T_DIR && !ip.isdirempty() {
                ip.unlockput();
            } else {
                let bytes_write =
                    dp.write(0, &mut de as *mut Dirent as usize, off, DIRENT_SIZE as u32);
                assert_eq!(bytes_write, Ok(DIRENT_SIZE), "unlink: writei");
                if ip.typ == T_DIR {
                    dp.nlink -= 1;
                    dp.update();
                }
                dp.unlockput();
                ip.nlink -= 1;
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

// TODO: Returning lockguard can be dangerous. ('static lifetime too)
unsafe fn create(path: &Path, typ: i16, major: u16, minor: u16) -> Result<InodeGuard<'static>, ()> {
    let (ptr, name) = path.nameiparent()?;
    let mut dp = (*ptr).lock();
    // TODO: use other Result related functions
    if let Ok((ptr2, _)) = dp.dirlookup(&name) {
        dp.unlockput();
        let ip = (*ptr2).lock();
        if typ == T_FILE && (ip.typ == T_FILE || ip.typ == T_DEVICE) {
            return Ok(ip);
        }
        ip.unlockput();
        return Err(());
    }
    let ptr2 = Inode::alloc((*ptr).dev, typ);
    if ptr2.is_null() {
        panic!("create: Inode::alloc");
    }
    let mut ip = (*ptr2).lock();
    ip.major = major;
    ip.minor = minor;
    ip.nlink = 1;
    ip.update();

    // Create . and .. entries.
    if typ == T_DIR {
        // for ".."
        dp.nlink += 1;
        dp.update();

        // No ip->nlink++ for ".": avoid cyclic ref count.
        ip.dirlink(FileName::from_bytes(b"."), (*ptr2).inum)
            .and_then(|_| ip.dirlink(FileName::from_bytes(b".."), (*ptr).inum))
            .expect("create dots");
    }
    dp.dirlink(&name, (*ptr2).inum).expect("create: dirlink");
    dp.unlockput();
    Ok(ip)
}

#[allow(clippy::cast_ref_to_mut)]
pub unsafe fn sys_open() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let path = ok_or!(argstr(0, &mut path), return usize::MAX);
    let path = Path::new(path);
    let omode = ok_or!(argint(1), return usize::MAX);
    begin_op();
    let omode = FcntlFlags::from_bits_truncate(omode);
    let ip = if omode.contains(FcntlFlags::O_CREATE) {
        ok_or!(create(path, T_FILE, 0, 0), {
            end_op();
            return usize::MAX;
        })
    } else {
        let ptr = ok_or!(path.namei(), {
            end_op();
            return usize::MAX;
        });
        let ip = (*ptr).lock();
        if ip.typ == T_DIR && omode != FcntlFlags::O_RDONLY {
            ip.unlockput();
            end_op();
            return usize::MAX;
        }
        ip
    };
    if ip.typ == T_DEVICE && (ip.major as usize >= NDEV) {
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

    if ip.typ == T_DEVICE {
        (*f).typ = FileType::Device {
            //TODO : Use better code
            ip: *(&ip.ptr as *const _ as *mut _),
            major: ip.major,
        };
    } else {
        (*f).typ = FileType::Inode {
            ip: *(&ip.ptr as *const _ as *mut _),
            off: 0,
        };
    }

    drop(ip);
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
    let ip = ok_or!(create(Path::new(path), T_DIR, 0, 0), {
        end_op();
        return usize::MAX;
    });
    ip.unlockput();
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
    let ip = ok_or!(
        create(Path::new(path), T_DEVICE, major, minor),
        return usize::MAX
    );
    ip.unlockput();
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
    let ip = (*ptr).lock();
    if ip.typ != T_DIR {
        ip.unlockput();
        end_op();
        return usize::MAX;
    }
    drop(ip);
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
