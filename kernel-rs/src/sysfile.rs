//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.
use crate::libc;
use crate::{
    exec::exec,
    fcntl::FcntlFlags,
    file::{Inode, RcFile},
    fs::{dirlink, dirlookup, namecmp, namei, nameiparent},
    fs::{Dirent, DIRSIZ},
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
    vm::copyout,
};
use core::mem;
use core::ptr;

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
    ok_or!((*f).read(p, n), return usize::MAX)
}

pub unsafe fn sys_write() -> usize {
    let (_, f) = ok_or!(argfd(0), return usize::MAX);
    let n = ok_or!(argint(2), return usize::MAX);
    let p = ok_or!(argaddr(1), return usize::MAX);
    ok_or!((*f).write(p, n), return usize::MAX)
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
    ok_or!((*f).stat(st), return usize::MAX)
}

/// Create the path new as a link to the same inode as old.
pub unsafe fn sys_link() -> usize {
    let mut name: [u8; DIRSIZ] = [0; DIRSIZ];
    let mut new: [u8; MAXPATH as usize] = [0; MAXPATH];
    let mut old: [u8; MAXPATH as usize] = [0; MAXPATH];
    let _ = ok_or!(argstr(0, old.as_mut_ptr(), MAXPATH), return usize::MAX);
    let _ = ok_or!(argstr(1, new.as_mut_ptr(), MAXPATH), return usize::MAX);
    begin_op();
    let mut ip: *mut Inode = namei(old.as_mut_ptr());
    if ip.is_null() {
        end_op();
        return usize::MAX;
    }
    (*ip).lock();
    if (*ip).typ as i32 == T_DIR {
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    }
    (*ip).nlink += 1;
    (*ip).update();
    (*ip).unlock();
    let dp: *mut Inode = nameiparent(new.as_mut_ptr(), name.as_mut_ptr());
    if !dp.is_null() {
        (*dp).lock();
        if (*dp).dev != (*ip).dev || dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
            (*dp).unlockput();
        } else {
            (*dp).unlockput();
            (*ip).put();
            end_op();
            return 0;
        }
    }
    (*ip).lock();
    (*ip).nlink -= 1;
    (*ip).update();
    (*ip).unlockput();
    end_op();
    usize::MAX
}

/// Is the directory dp empty except for "." and ".." ?
unsafe fn isdirempty(dp: *mut Inode) -> i32 {
    let mut de: Dirent = Default::default();
    let mut off = (2usize).wrapping_mul(mem::size_of::<Dirent>()) as i32;
    while (off as u32) < (*dp).size {
        if (*dp).read(
            0,
            &mut de as *mut Dirent as usize,
            off as u32,
            mem::size_of::<Dirent>() as u32,
        ) as usize
            != mem::size_of::<Dirent>()
        {
            panic!("isdirempty: readi");
        }
        if de.inum as i32 != 0 {
            return 0;
        }
        off = (off as usize).wrapping_add(mem::size_of::<Dirent>()) as i32
    }
    1
}

pub unsafe fn sys_unlink() -> usize {
    let mut de: Dirent = Default::default();
    let mut name: [u8; DIRSIZ] = [0; DIRSIZ];
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut off: u32 = 0;
    let _ = ok_or!(argstr(0, path.as_mut_ptr(), MAXPATH), return usize::MAX);
    begin_op();
    let mut dp: *mut Inode = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return usize::MAX;
    }
    (*dp).lock();

    // Cannot unlink "." or "..".
    if !(namecmp(name.as_mut_ptr(), b".\x00" as *const u8) == 0
        || namecmp(name.as_mut_ptr(), b"..\x00" as *const u8) == 0)
    {
        let mut ip: *mut Inode = dirlookup(dp, name.as_mut_ptr(), &mut off);
        if !ip.is_null() {
            (*ip).lock();
            if ((*ip).nlink as i32) < 1 {
                panic!("unlink: nlink < 1");
            }
            if (*ip).typ as i32 == T_DIR && isdirempty(ip) == 0 {
                (*ip).unlockput();
            } else {
                ptr::write_bytes(&mut de as *mut Dirent, 0, 1);
                if (*dp).write(
                    0,
                    &mut de as *mut Dirent as usize,
                    off,
                    mem::size_of::<Dirent>() as u32,
                ) as usize
                    != mem::size_of::<Dirent>()
                {
                    panic!("unlink: writei");
                }
                if (*ip).typ as i32 == T_DIR {
                    (*dp).nlink -= 1;
                    (*dp).update();
                }
                (*dp).unlockput();
                (*ip).nlink -= 1;
                (*ip).update();
                (*ip).unlockput();
                end_op();
                return 0;
            }
        }
    }
    (*dp).unlockput();
    end_op();
    usize::MAX
}

unsafe fn create(path: *mut u8, typ: i16, major: i16, minor: i16) -> *mut Inode {
    let mut name: [u8; DIRSIZ] = [0; DIRSIZ];
    let mut dp: *mut Inode = nameiparent(path, name.as_mut_ptr());
    if dp.is_null() {
        return ptr::null_mut();
    }
    (*dp).lock();
    let mut ip: *mut Inode = dirlookup(dp, name.as_mut_ptr(), ptr::null_mut());
    if !ip.is_null() {
        (*dp).unlockput();
        (*ip).lock();
        if typ as i32 == T_FILE && ((*ip).typ as i32 == T_FILE || (*ip).typ as i32 == T_DEVICE) {
            return ip;
        }
        (*ip).unlockput();
        return ptr::null_mut();
    }
    ip = Inode::alloc((*dp).dev, typ);
    if ip.is_null() {
        panic!("create: Inode::alloc");
    }
    (*ip).lock();
    (*ip).major = major;
    (*ip).minor = minor;
    (*ip).nlink = 1;
    (*ip).update();

    // Create . and .. entries.
    if typ as i32 == T_DIR {
        // for ".."
        (*dp).nlink += 1;
        (*dp).update();

        // No ip->nlink++ for ".": avoid cyclic ref count.
        if dirlink(ip, b".\x00" as *const u8 as *mut u8, (*ip).inum) < 0
            || dirlink(ip, b"..\x00" as *const u8 as *mut u8, (*dp).inum) < 0
        {
            panic!("create dots");
        }
    }
    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
        panic!("create: dirlink");
    }
    (*dp).unlockput();
    ip
}

pub unsafe fn sys_open() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let ip: *mut Inode;
    let _ = ok_or!(argstr(0, path.as_mut_ptr(), MAXPATH), return usize::MAX);
    let omode = ok_or!(argint(1), return usize::MAX);
    begin_op();
    let omode = FcntlFlags::from_bits_truncate(omode);
    if omode.contains(FcntlFlags::O_CREATE) {
        ip = create(path.as_mut_ptr(), T_FILE as i16, 0, 0);
        if ip.is_null() {
            end_op();
            return usize::MAX;
        }
    } else {
        ip = namei(path.as_mut_ptr());
        if ip.is_null() {
            end_op();
            return usize::MAX;
        }
        (*ip).lock();
        if (*ip).typ as i32 == T_DIR && omode != FcntlFlags::O_RDONLY {
            (*ip).unlockput();
            end_op();
            return usize::MAX;
        }
    }
    if (*ip).typ as i32 == T_DEVICE && (((*ip).major) < 0 || (*ip).major as usize >= NDEV) {
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    }
    let f = some_or!(RcFile::alloc(), {
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    });

    let fd = match f.fdalloc() {
        Ok(fd) => fd,
        Err(f) => {
            drop(f);
            (*ip).unlockput();
            end_op();
            return usize::MAX;
        }
    };

    let f = (*myproc()).open_files[fd as usize].as_mut().unwrap();

    if (*ip).typ as i32 == T_DEVICE {
        (*f).set_filetype_device(ip, (*ip).major);
    } else {
        (*f).set_filetype_inode(ip);
    }
    (*f).set_readable(!omode.intersects(FcntlFlags::O_WRONLY));
    (*f).set_writable(omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR));

    (*ip).unlock();
    end_op();
    fd as usize
}

pub unsafe fn sys_mkdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut ip: *mut Inode = ptr::null_mut();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH).is_err() || {
        ip = create(path.as_mut_ptr(), T_DIR as i16, 0, 0);
        ip.is_null()
    } {
        end_op();
        return usize::MAX;
    }
    (*ip).unlockput();
    end_op();
    0
}

pub unsafe fn sys_mknod() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    begin_op();
    let _end_op = scopeguard::guard((), |_| {
        end_op();
    });
    let _ = ok_or!(argstr(0, path.as_mut_ptr(), MAXPATH), return usize::MAX);
    let major = ok_or!(argint(1), return usize::MAX);
    let minor = ok_or!(argint(2), return usize::MAX);
    let ip = create(
        path.as_mut_ptr(),
        T_DEVICE as i16,
        major as i16,
        minor as i16,
    );
    if ip.is_null() {
        return usize::MAX;
    }
    (*ip).unlockput();
    0
}

pub unsafe fn sys_chdir() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut ip: *mut Inode = ptr::null_mut();
    let mut p: *mut Proc = myproc();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH).is_err() || {
        ip = namei(path.as_mut_ptr());
        ip.is_null()
    } {
        end_op();
        return usize::MAX;
    }
    (*ip).lock();
    if (*ip).typ as i32 != T_DIR {
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    }
    (*ip).unlock();
    (*(*p).cwd).put();
    end_op();
    (*p).cwd = ip;
    0
}

pub unsafe fn sys_exec() -> usize {
    let mut path: [u8; MAXPATH] = [0; MAXPATH];
    let mut argv: [*mut u8; MAXARG] = [ptr::null_mut(); MAXARG];
    let _ = ok_or!(argstr(0, path.as_mut_ptr(), MAXPATH), return usize::MAX);
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

        if fetchstr(uarg, *arg, PGSIZE) < 0 {
            break;
        }
    }

    let ret = if success {
        exec(path.as_mut_ptr(), argv.as_mut_ptr()) as usize
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

    if copyout(
        (*p).pagetable,
        fdarray,
        &mut fd0 as *mut i32 as *mut u8,
        mem::size_of::<i32>(),
    ) < 0
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(mem::size_of::<i32>()),
            &mut fd1 as *mut i32 as *mut u8,
            mem::size_of::<i32>(),
        ) < 0
    {
        (*p).open_files[fd0 as usize] = None;
        (*p).open_files[fd1 as usize] = None;
        return usize::MAX;
    }
    0
}
