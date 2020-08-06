/// File-system system calls.
/// Mostly argument checking, since we don't trust
/// user code, and calls into file.c and fs.c.
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
    pipe::pipealloc,
    printf::panic,
    proc::{myproc, proc_0},
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
        let mut fd: i32 = 0; // user pointer to struct stat
        let mut p: *mut proc_0 = myproc();
        while fd < NOFILE {
            if (*p).ofile[fd as usize].is_null() {
                (*p).ofile[fd as usize] = self;
                return fd;
            }
            fd += 1
        }
        -1
    }
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe fn argfd(mut n: i32, mut pfd: *mut i32, mut pf: *mut *mut File) -> i32 {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argint(n, &mut fd) < 0 {
        return -1;
    }
    if fd < 0 || fd >= NOFILE || {
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
    let mut fd: i32 = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 {
        return -1 as _;
    }
    fd = (*f).fdalloc();
    if fd < 0 {
        return -1 as _;
    }
    (*f).dup();
    fd as usize
}

pub unsafe fn sys_read() -> usize {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: usize = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 || argint(2, &mut n) < 0 || argaddr(1, &mut p) < 0 {
        return -1 as _;
    }
    (*f).read(p, n) as usize
}

pub unsafe fn sys_write() -> usize {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: usize = 0;
    if argfd(0, ptr::null_mut(), &mut f) < 0 || argint(2, &mut n) < 0 || argaddr(1, &mut p) < 0 {
        return -1 as _;
    }
    (*f).write(p, n) as usize
}

pub unsafe fn sys_close() -> usize {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argfd(0, &mut fd, &mut f) < 0 {
        return -1 as _;
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
        return -1 as _;
    }
    (*f).stat(st) as usize
}

/// Create the path new as a link to the same inode as old.
pub unsafe fn sys_link() -> usize {
    let mut name: [libc::c_char; DIRSIZ] = [0; DIRSIZ];
    let mut new: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut old: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut dp: *mut Inode = ptr::null_mut();
    let mut ip: *mut Inode = ptr::null_mut();
    if argstr(0, old.as_mut_ptr(), MAXPATH) < 0 || argstr(1, new.as_mut_ptr(), MAXPATH) < 0 {
        return -1 as _;
    }
    begin_op();
    ip = namei(old.as_mut_ptr());
    if ip.is_null() {
        end_op();
        return -1 as _;
    }
    (*ip).lock();
    if (*ip).typ as i32 == T_DIR {
        (*ip).unlockput();
        end_op();
        return -1 as _;
    }
    (*ip).nlink += 1;
    (*ip).update();
    (*ip).unlock();
    dp = nameiparent(new.as_mut_ptr(), name.as_mut_ptr());
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
    -1 as _
}

/// Is the directory dp empty except for "." and ".." ?
unsafe fn isdirempty(mut dp: *mut Inode) -> i32 {
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
            panic(
                b"isdirempty: readi\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
        }
        if de.inum as i32 != 0 {
            return 0;
        }
        off = (off as usize).wrapping_add(::core::mem::size_of::<Dirent>()) as i32
    }
    1
}

pub unsafe fn sys_unlink() -> usize {
    let mut ip: *mut Inode = ptr::null_mut();
    let mut dp: *mut Inode = ptr::null_mut();
    let mut de: Dirent = Default::default();
    let mut name: [libc::c_char; DIRSIZ] = [0; DIRSIZ];
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut off: u32 = 0;
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 {
        return -1 as _;
    }
    begin_op();
    dp = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return -1 as _;
    }
    (*dp).lock();

    // Cannot unlink "." or "..".
    if !(namecmp(
        name.as_mut_ptr(),
        b".\x00" as *const u8 as *const libc::c_char,
    ) == 0
        || namecmp(
            name.as_mut_ptr(),
            b"..\x00" as *const u8 as *const libc::c_char,
        ) == 0)
    {
        ip = dirlookup(dp, name.as_mut_ptr(), &mut off);
        if !ip.is_null() {
            (*ip).lock();
            if ((*ip).nlink as i32) < 1 {
                panic(
                    b"unlink: nlink < 1\x00" as *const u8 as *const libc::c_char
                        as *mut libc::c_char,
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
                        b"unlink: writei\x00" as *const u8 as *const libc::c_char
                            as *mut libc::c_char,
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
    -1 as _
}

unsafe fn create(
    mut path: *mut libc::c_char,
    mut typ: i16,
    mut major: i16,
    mut minor: i16,
) -> *mut Inode {
    let mut ip: *mut Inode = ptr::null_mut();
    let mut dp: *mut Inode = ptr::null_mut();
    let mut name: [libc::c_char; DIRSIZ] = [0; DIRSIZ];
    dp = nameiparent(path, name.as_mut_ptr());
    if dp.is_null() {
        return ptr::null_mut();
    }
    (*dp).lock();
    ip = dirlookup(dp, name.as_mut_ptr(), ptr::null_mut());
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
        panic(b"create: Inode::alloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*ip).lock();
    (*ip).major = major;
    (*ip).minor = minor;
    (*ip).nlink = 1 as i16;
    (*ip).update();

    // Create . and .. entries.
    if typ as i32 == T_DIR {
        // for ".."
        (*dp).nlink += 1;
        (*dp).update();

        // No ip->nlink++ for ".": avoid cyclic ref count.
        if dirlink(
            ip,
            b".\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            (*ip).inum,
        ) < 0
            || dirlink(
                ip,
                b"..\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                (*dp).inum,
            ) < 0
        {
            panic(b"create dots\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
    }
    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
        panic(b"create: dirlink\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*dp).unlockput();
    ip
}

pub unsafe fn sys_open() -> usize {
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut fd: i32 = 0;
    let mut omode: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    let mut ip: *mut Inode = ptr::null_mut();
    let mut n: i32 = 0;
    n = argstr(0, path.as_mut_ptr(), MAXPATH);
    if n < 0 || argint(1, &mut omode) < 0 {
        return -1 as _;
    }
    begin_op();
    let omode = FcntlFlags::from_bits_truncate(omode);
    if omode.contains(FcntlFlags::O_CREATE) {
        ip = create(path.as_mut_ptr(), T_FILE as i16, 0 as i16, 0 as i16);
        if ip.is_null() {
            end_op();
            return -1 as _;
        }
    } else {
        ip = namei(path.as_mut_ptr());
        if ip.is_null() {
            end_op();
            return -1 as _;
        }
        (*ip).lock();
        if (*ip).typ as i32 == T_DIR && omode != FcntlFlags::O_RDONLY {
            (*ip).unlockput();
            end_op();
            return -1 as _;
        }
    }
    if (*ip).typ as i32 == T_DEVICE && (((*ip).major as i32) < 0 || (*ip).major as i32 >= NDEV) {
        (*ip).unlockput();
        end_op();
        return -1 as _;
    }
    f = File::alloc();
    if f.is_null() || {
        fd = (*f).fdalloc();
        (fd) < 0
    } {
        if !f.is_null() {
            (*f).close();
        }
        (*ip).unlockput();
        end_op();
        return -1 as _;
    }
    if (*ip).typ as i32 == T_DEVICE {
        (*f).typ = FD_DEVICE;
        (*f).major = (*ip).major
    } else {
        (*f).typ = FD_INODE;
        (*f).off = 0 as u32
    }
    (*f).ip = ip;
    (*f).readable = (!omode.intersects(FcntlFlags::O_WRONLY)) as i32 as libc::c_char;
    (*f).writable =
        omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR) as i32 as libc::c_char;
    (*ip).unlock();
    end_op();
    fd as usize
}

pub unsafe fn sys_mkdir() -> usize {
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut ip: *mut Inode = ptr::null_mut();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 || {
        ip = create(path.as_mut_ptr(), T_DIR as i16, 0 as i16, 0 as i16);
        ip.is_null()
    } {
        end_op();
        return -1 as _;
    }
    (*ip).unlockput();
    end_op();
    0
}

pub unsafe fn sys_mknod() -> usize {
    let mut ip: *mut Inode = ptr::null_mut();
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut major: i32 = 0;
    let mut minor: i32 = 0;
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0
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
        return -1 as _;
    }
    (*ip).unlockput();
    end_op();
    0
}

pub unsafe fn sys_chdir() -> usize {
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut ip: *mut Inode = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 || {
        ip = namei(path.as_mut_ptr());
        ip.is_null()
    } {
        end_op();
        return -1 as _;
    }
    (*ip).lock();
    if (*ip).typ as i32 != T_DIR {
        (*ip).unlockput();
        end_op();
        return -1 as _;
    }
    (*ip).unlock();
    (*(*p).cwd).put();
    end_op();
    (*p).cwd = ip;
    0
}

pub unsafe fn sys_exec() -> usize {
    let mut current_block: usize;
    let mut path: [libc::c_char; MAXPATH as usize] = [0; MAXPATH as usize];
    let mut argv: [*mut libc::c_char; MAXARG] = [ptr::null_mut(); MAXARG];
    let mut i: i32 = 0;
    let mut uargv: usize = 0;
    let mut uarg: usize = 0;
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 || argaddr(1, &mut uargv) < 0 {
        return -1 as _;
    }
    ptr::write_bytes(argv.as_mut_ptr(), 0, 1);
    loop {
        if i as usize
            >= (::core::mem::size_of::<[*mut libc::c_char; 32]>())
                .wrapping_div(::core::mem::size_of::<*mut libc::c_char>())
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
            argv[i as usize] = kalloc() as *mut libc::c_char;
            if argv[i as usize].is_null() {
                panic(
                    b"sys_exec kalloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                );
            }
            if fetchstr(uarg, argv[i as usize], PGSIZE) < 0 {
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
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>())
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>())
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            -1 as _
        }
        _ => {
            let ret = exec(path.as_mut_ptr(), argv.as_mut_ptr());
            i = 0;
            while (i as usize)
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>())
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>())
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            ret as usize
        }
    }
}

// user pointer to array of two integers
pub unsafe fn sys_pipe() -> usize {
    let mut fdarray: usize = 0;
    let mut rf: *mut File = ptr::null_mut();
    let mut wf: *mut File = ptr::null_mut();
    let mut fd0: i32 = 0;
    let mut fd1: i32 = 0;
    let mut p: *mut proc_0 = myproc();
    if argaddr(0, &mut fdarray) < 0 {
        return -1 as _;
    }
    if pipealloc(&mut rf, &mut wf) < 0 {
        return -1 as _;
    }
    fd0 = -1;
    fd0 = (*rf).fdalloc();
    if fd0 < 0 || {
        fd1 = (*wf).fdalloc();
        (fd1) < 0
    } {
        if fd0 >= 0 {
            (*p).ofile[fd0 as usize] = ptr::null_mut()
        }
        (*rf).close();
        (*wf).close();
        return -1 as _;
    }
    if copyout(
        (*p).pagetable,
        fdarray,
        &mut fd0 as *mut i32 as *mut libc::c_char,
        ::core::mem::size_of::<i32>(),
    ) < 0
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(::core::mem::size_of::<i32>()),
            &mut fd1 as *mut i32 as *mut libc::c_char,
            ::core::mem::size_of::<i32>(),
        ) < 0
    {
        (*p).ofile[fd0 as usize] = ptr::null_mut();
        (*p).ofile[fd1 as usize] = ptr::null_mut();
        (*rf).close();
        (*wf).close();
        return -1 as _;
    }
    0
}
