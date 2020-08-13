//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.
use crate::libc;
use crate::{
    exec::exec,
    fcntl::FcntlFlags,
    file::{File, Inode},
    fs::{dirlink, dirlookup, namecmp, namei, nameiparent},
    fs::{Dirent, DIRSIZ},
    kalloc::{kalloc, kfree},
    log::{begin_op, end_op},
    param::{MAXARG, MAXPATH, NDEV, NOFILE},
    pipe::Pipe,
    printf::panic,
    proc::{myproc, Proc},
    riscv::PGSIZE,
    stat::{T_DEVICE, T_DIR, T_FILE},
    syscall::{argaddr, argint, argstr, fetchaddr, fetchstr},
    vm::copyout,
};
use core::ptr;
pub const FD_DEVICE: u32 = 3;
pub const FD_INODE: u32 = 2;
pub const FD_PIPE: u32 = 1;
pub const FD_NONE: u32 = 0;

impl File {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    unsafe fn fdalloc(&mut self) -> i32 {
        let mut p: *mut Proc = myproc();
        for fd in 0..NOFILE {
            // user pointer to struct stat
            if (*p).ofile[fd].is_null() {
                (*p).ofile[fd] = self;
                return fd as i32;
            }
        }
        -1
    }
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe fn argfd(n: i32, pfd: *mut i32, pf: *mut *mut File) -> i32 {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argint(n, &mut fd) < 0 {
        return -1;
    }
    if fd < 0 || fd >= NOFILE as i32 || {
        f = (*myproc()).ofile[fd as usize];
        f.is_null()
    } {
        return -1;
    }
    if !pfd.is_null() {
        *pfd = fd
    }
    if !pf.is_null() {
        *pf = f
    }
    0
}

pub unsafe fn sys_dup() -> usize {
    let mut f: *mut File = ptr::null_mut();
    if argfd(0, ptr::null_mut(), &mut f) < 0 {
        return usize::MAX;
    }
    let fd: i32 = (*f).fdalloc();
    if fd < 0 {
        return usize::MAX;
    }
    (*f).dup();
    fd as usize
}

pub unsafe fn sys_read() -> usize {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: usize = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 || argint(2, &mut n) < 0 || argaddr(1, &mut p) < 0 {
        return usize::MAX;
    }
    (*f).read(p, n) as usize
}

pub unsafe fn sys_write() -> usize {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: usize = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 || argint(2, &mut n) < 0 || argaddr(1, &mut p) < 0 {
        return usize::MAX;
    }
    (*f).write(p, n) as usize
}

pub unsafe fn sys_close() -> usize {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argfd(0, &mut fd, &mut f) < 0 {
        return usize::MAX;
    }
    let fresh0 = &mut (*myproc()).ofile[fd as usize];
    *fresh0 = ptr::null_mut();
    (*f).close();
    0
}

pub unsafe fn sys_fstat() -> usize {
    let mut f: *mut File = ptr::null_mut();

    // user pointer to struct stat
    let mut st: usize = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 || argaddr(1, &mut st) < 0 {
        return usize::MAX;
    }
    (*f).stat(st) as usize
}

/// Create the path new as a link to the same inode as old.
pub unsafe fn sys_link() -> usize {
    let mut name: [libc::CChar; DIRSIZ] = [0; DIRSIZ];
    let mut new: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut old: [libc::CChar; MAXPATH] = [0; MAXPATH];
    if argstr(0, old.as_mut_ptr(), MAXPATH as i32) < 0
        || argstr(1, new.as_mut_ptr(), MAXPATH as i32) < 0
    {
        return usize::MAX;
    }
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
    let mut off = (2usize).wrapping_mul(::core::mem::size_of::<Dirent>()) as i32;
    while (off as u32) < (*dp).size {
        if (*dp).read(
            0,
            &mut de as *mut Dirent as usize,
            off as u32,
            ::core::mem::size_of::<Dirent>() as u32,
        ) as usize
            != ::core::mem::size_of::<Dirent>()
        {
            panic(b"isdirempty: readi\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        if de.inum as i32 != 0 {
            return 0;
        }
        off = (off as usize).wrapping_add(::core::mem::size_of::<Dirent>()) as i32
    }
    1
}

pub unsafe fn sys_unlink() -> usize {
    let mut de: Dirent = Default::default();
    let mut name: [libc::CChar; DIRSIZ] = [0; DIRSIZ];
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut off: u32 = 0;
    if argstr(0, path.as_mut_ptr(), MAXPATH as i32) < 0 {
        return usize::MAX;
    }
    begin_op();
    let mut dp: *mut Inode = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return usize::MAX;
    }
    (*dp).lock();

    // Cannot unlink "." or "..".
    if !(namecmp(
        name.as_mut_ptr(),
        b".\x00" as *const u8 as *const libc::CChar,
    ) == 0
        || namecmp(
            name.as_mut_ptr(),
            b"..\x00" as *const u8 as *const libc::CChar,
        ) == 0)
    {
        let mut ip: *mut Inode = dirlookup(dp, name.as_mut_ptr(), &mut off);
        if !ip.is_null() {
            (*ip).lock();
            if ((*ip).nlink as i32) < 1 {
                panic(
                    b"unlink: nlink < 1\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
                );
            }
            if (*ip).typ as i32 == T_DIR && isdirempty(ip) == 0 {
                (*ip).unlockput();
            } else {
                ptr::write_bytes(&mut de as *mut Dirent, 0, 1);
                if (*dp).write(
                    0,
                    &mut de as *mut Dirent as usize,
                    off,
                    ::core::mem::size_of::<Dirent>() as u32,
                ) as usize
                    != ::core::mem::size_of::<Dirent>()
                {
                    panic(
                        b"unlink: writei\x00" as *const u8 as *const libc::CChar
                            as *mut libc::CChar,
                    );
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

unsafe fn create(path: *mut libc::CChar, typ: i16, major: i16, minor: i16) -> *mut Inode {
    let mut name: [libc::CChar; DIRSIZ] = [0; DIRSIZ];
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
        panic(b"create: Inode::alloc\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
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
        if dirlink(
            ip,
            b".\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
            (*ip).inum,
        ) < 0
            || dirlink(
                ip,
                b"..\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
                (*dp).inum,
            ) < 0
        {
            panic(b"create dots\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
    }
    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
        panic(b"create: dirlink\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }
    (*dp).unlockput();
    ip
}

pub unsafe fn sys_open() -> usize {
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut fd: i32 = 0;
    let mut omode: i32 = 0;
    let ip: *mut Inode;
    let n: i32 = argstr(0, path.as_mut_ptr(), MAXPATH as i32);
    if n < 0 || argint(1, &mut omode) < 0 {
        return usize::MAX;
    }
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
    if (*ip).typ as i32 == T_DEVICE && (((*ip).major as i32) < 0 || (*ip).major as i32 >= NDEV) {
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    }
    let mut f: *mut File = File::alloc();
    if f.is_null() || {
        fd = (*f).fdalloc();
        (fd) < 0
    } {
        if !f.is_null() {
            (*f).close();
        }
        (*ip).unlockput();
        end_op();
        return usize::MAX;
    }
    if (*ip).typ as i32 == T_DEVICE {
        (*f).typ = FD_DEVICE;
        (*f).major = (*ip).major
    } else {
        (*f).typ = FD_INODE;
        (*f).off = 0
    }
    (*f).ip = ip;
    (*f).readable = (!omode.intersects(FcntlFlags::O_WRONLY)) as i32 as libc::CChar;
    (*f).writable =
        omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR) as i32 as libc::CChar;
    (*ip).unlock();
    end_op();
    fd as usize
}

pub unsafe fn sys_mkdir() -> usize {
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut ip: *mut Inode = ptr::null_mut();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH as i32) < 0 || {
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
    let mut ip: *mut Inode = ptr::null_mut();
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut major: i32 = 0;
    let mut minor: i32 = 0;
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH as i32) < 0
        || argint(1, &mut major) < 0
        || argint(2, &mut minor) < 0
        || {
            ip = create(
                path.as_mut_ptr(),
                T_DEVICE as i16,
                major as i16,
                minor as i16,
            );
            ip.is_null()
        }
    {
        end_op();
        return usize::MAX;
    }
    (*ip).unlockput();
    end_op();
    0
}

pub unsafe fn sys_chdir() -> usize {
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut ip: *mut Inode = ptr::null_mut();
    let mut p: *mut Proc = myproc();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH as i32) < 0 || {
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
    let current_block: usize;
    let mut path: [libc::CChar; MAXPATH] = [0; MAXPATH];
    let mut argv: [*mut libc::CChar; MAXARG] = [ptr::null_mut(); MAXARG];
    let mut i: i32 = 0;
    let mut uargv: usize = 0;
    let mut uarg: usize = 0;
    if argstr(0, path.as_mut_ptr(), MAXPATH as i32) < 0 || argaddr(1, &mut uargv) < 0 {
        return usize::MAX;
    }
    ptr::write_bytes(argv.as_mut_ptr(), 0, 1);
    loop {
        if i as usize
            >= (::core::mem::size_of::<[*mut libc::CChar; 32]>())
                .wrapping_div(::core::mem::size_of::<*mut libc::CChar>())
        {
            current_block = 12646643519710607562;
            break;
        }
        if fetchaddr(
            uargv.wrapping_add((::core::mem::size_of::<usize>()).wrapping_mul(i as usize)),
            &mut uarg as *mut usize,
        ) < 0
        {
            current_block = 12646643519710607562;
            break;
        }
        if uarg == 0 {
            argv[i as usize] = ptr::null_mut();
            current_block = 6009453772311597924;
            break;
        } else {
            argv[i as usize] = kalloc() as *mut libc::CChar;
            if argv[i as usize].is_null() {
                panic(
                    b"sys_exec kalloc\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
                );
            }
            if fetchstr(uarg, argv[i as usize], PGSIZE as i32) < 0 {
                current_block = 12646643519710607562;
                break;
            }
            i += 1
        }
    }
    match current_block {
        12646643519710607562 => {
            i = 0;
            while (i as usize)
                < (::core::mem::size_of::<[*mut libc::CChar; 32]>())
                    .wrapping_div(::core::mem::size_of::<*mut libc::CChar>())
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::CVoid);
                i += 1
            }
            usize::MAX
        }
        _ => {
            let ret = exec(path.as_mut_ptr(), argv.as_mut_ptr());
            i = 0;
            while (i as usize)
                < (::core::mem::size_of::<[*mut libc::CChar; 32]>())
                    .wrapping_div(::core::mem::size_of::<*mut libc::CChar>())
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::CVoid);
                i += 1
            }
            ret as usize
        }
    }
}

pub unsafe fn sys_pipe() -> usize {
    // user pointer to array of two integers
    let mut fdarray: usize = 0;

    let mut rf: *mut File = ptr::null_mut();
    let mut wf: *mut File = ptr::null_mut();
    let mut fd1: i32 = 0;
    let mut p: *mut Proc = myproc();
    if argaddr(0, &mut fdarray) < 0 {
        return usize::MAX;
    }
    if Pipe::alloc(&mut rf, &mut wf) < 0 {
        return usize::MAX;
    }
    let mut fd0: i32 = (*rf).fdalloc();
    if fd0 < 0 || {
        fd1 = (*wf).fdalloc();
        (fd1) < 0
    } {
        if fd0 >= 0 {
            (*p).ofile[fd0 as usize] = ptr::null_mut()
        }
        (*rf).close();
        (*wf).close();
        return usize::MAX;
    }
    if copyout(
        (*p).pagetable,
        fdarray,
        &mut fd0 as *mut i32 as *mut libc::CChar,
        ::core::mem::size_of::<i32>(),
    ) < 0
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(::core::mem::size_of::<i32>()),
            &mut fd1 as *mut i32 as *mut libc::CChar,
            ::core::mem::size_of::<i32>(),
        ) < 0
    {
        (*p).ofile[fd0 as usize] = ptr::null_mut();
        (*p).ofile[fd1 as usize] = ptr::null_mut();
        (*rf).close();
        (*wf).close();
        return usize::MAX;
    }
    0
}
