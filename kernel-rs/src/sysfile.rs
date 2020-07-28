use crate::{
    exec::exec,
    file::{filealloc, fileclose, filedup, fileread, filestat, filewrite},
    file::{inode, File},
    fs::dirent,
    fs::{
        dirlink, dirlookup, ialloc, ilock, iput, iunlock, iunlockput, iupdate, namecmp, namei,
        nameiparent, readi, writei,
    },
    kalloc::{kalloc, kfree},
    libc,
    log::{begin_op, end_op},
    pipe::pipealloc,
    printf::panic,
    proc::{myproc, proc_0},
    string::memset,
    syscall::{argaddr, argint, argstr, fetchaddr, fetchstr},
    vm::copyout,
};
use core::ptr;
pub type pagetable_t = *mut u64;
pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;
pub const PGSIZE: i32 = 4096;
// maximum number of processes
// maximum number of CPUs
pub const NOFILE: i32 = 16;
// open files per process
// open files per system
// maximum number of active i-nodes
pub const NDEV: i32 = 10;
// maximum major device number
// device number of file system root disk
// max exec arguments
// max # of blocks any FS op writes
// max data blocks in on-disk log
// size of disk block cache
// size of file system in blocks
pub const MAXPATH: i32 = 128;
pub const T_DIR: i32 = 1;
// Directory
pub const T_FILE: i32 = 2;
// File
pub const T_DEVICE: i32 = 3;
pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 0x1;
pub const O_RDWR: i32 = 0x2;
pub const O_CREATE: i32 = 0x200;

/// File-system system calls.
/// Mostly argument checking, since we don't trust
/// user code, and calls into file.c and fs.c.

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe extern "C" fn argfd(mut n: i32, mut pfd: *mut i32, mut pf: *mut *mut File) -> i32 {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argint(n, &mut fd) < 0 as i32 {
        return -(1 as i32);
    }
    if fd < 0 as i32 || fd >= NOFILE || {
        f = (*myproc()).ofile[fd as usize];
        f.is_null()
    } {
        return -(1 as i32);
    }
    if !pfd.is_null() {
        *pfd = fd
    }
    if !pf.is_null() {
        *pf = f
    }
    0 as i32
}
/// Allocate a file descriptor for the given file.
/// Takes over file reference from caller on success.
unsafe extern "C" fn fdalloc(mut f: *mut File) -> i32 {
    let mut fd: i32 = 0; // user pointer to struct stat
    let mut p: *mut proc_0 = myproc();
    while fd < NOFILE {
        if (*p).ofile[fd as usize].is_null() {
            (*p).ofile[fd as usize] = f;
            return fd;
        }
        fd += 1
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn sys_dup() -> u64 {
    let mut f: *mut File = ptr::null_mut();
    let mut fd: i32 = 0;
    if argfd(0 as i32, ptr::null_mut(), &mut f) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    fd = fdalloc(f);
    if fd < 0 as i32 {
        return -(1 as i32) as u64;
    }
    filedup(f);
    fd as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_read() -> u64 {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: u64 = 0;
    if argfd(0 as i32, ptr::null_mut(), &mut f) < 0 as i32
        || argint(2 as i32, &mut n) < 0 as i32
        || argaddr(1 as i32, &mut p) < 0 as i32
    {
        return -(1 as i32) as u64;
    }
    fileread(f, p, n) as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_write() -> u64 {
    let mut f: *mut File = ptr::null_mut();
    let mut n: i32 = 0;
    let mut p: u64 = 0;
    if argfd(0 as i32, ptr::null_mut(), &mut f) < 0 as i32
        || argint(2 as i32, &mut n) < 0 as i32
        || argaddr(1 as i32, &mut p) < 0 as i32
    {
        return -(1 as i32) as u64;
    }
    filewrite(f, p, n) as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_close() -> u64 {
    let mut fd: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    if argfd(0 as i32, &mut fd, &mut f) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    let fresh0 = &mut (*myproc()).ofile[fd as usize];
    *fresh0 = ptr::null_mut();
    fileclose(f);
    0 as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_fstat() -> u64 {
    let mut f: *mut File = ptr::null_mut();
    let mut st: u64 = 0;
    if argfd(0 as i32, ptr::null_mut(), &mut f) < 0 as i32 || argaddr(1 as i32, &mut st) < 0 as i32
    {
        return -(1 as i32) as u64;
    }
    filestat(f, st) as u64
}
/// Create the path new as a link to the same inode as old.
#[no_mangle]
pub unsafe extern "C" fn sys_link() -> u64 {
    let mut name: [libc::c_char; 14] = [0; 14];
    let mut new: [libc::c_char; 128] = [0; 128];
    let mut old: [libc::c_char; 128] = [0; 128];
    let mut dp: *mut inode = ptr::null_mut();
    let mut ip: *mut inode = ptr::null_mut();
    if argstr(0 as i32, old.as_mut_ptr(), MAXPATH) < 0 as i32
        || argstr(1 as i32, new.as_mut_ptr(), MAXPATH) < 0 as i32
    {
        return -(1 as i32) as u64;
    }
    begin_op();
    ip = namei(old.as_mut_ptr());
    if ip.is_null() {
        end_op();
        return -(1 as i32) as u64;
    }
    ilock(ip);
    if (*ip).type_0 as i32 == T_DIR {
        iunlockput(ip);
        end_op();
        return -(1 as i32) as u64;
    }
    (*ip).nlink += 1;
    iupdate(ip);
    iunlock(ip);
    dp = nameiparent(new.as_mut_ptr(), name.as_mut_ptr());
    if !dp.is_null() {
        ilock(dp);
        if (*dp).dev != (*ip).dev || dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 as i32 {
            iunlockput(dp);
        } else {
            iunlockput(dp);
            iput(ip);
            end_op();
            return 0 as u64;
        }
    }
    ilock(ip);
    (*ip).nlink -= 1;
    iupdate(ip);
    iunlockput(ip);
    end_op();
    -(1 as i32) as u64
}
/// Is the directory dp empty except for "." and ".." ?
unsafe extern "C" fn isdirempty(mut dp: *mut inode) -> i32 {
    let mut off: i32 = 0;
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    off = (2 as u64).wrapping_mul(::core::mem::size_of::<dirent>() as u64) as i32;
    while (off as u32) < (*dp).size {
        if readi(
            dp,
            0 as i32,
            &mut de as *mut dirent as u64,
            off as u32,
            ::core::mem::size_of::<dirent>() as u64 as u32,
        ) as u64
            != ::core::mem::size_of::<dirent>() as u64
        {
            panic(
                b"isdirempty: readi\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
        }
        if de.inum as i32 != 0 as i32 {
            return 0 as i32;
        }
        off = (off as u64).wrapping_add(::core::mem::size_of::<dirent>() as u64) as i32 as i32
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn sys_unlink() -> u64 {
    let mut ip: *mut inode = ptr::null_mut();
    let mut dp: *mut inode = ptr::null_mut();
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    let mut name: [libc::c_char; 14] = [0; 14];
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut off: u32 = 0;
    if argstr(0 as i32, path.as_mut_ptr(), MAXPATH) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    begin_op();
    dp = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return -(1 as i32) as u64;
    }
    ilock(dp);
    // Cannot unlink "." or "..".
    if !(namecmp(
        name.as_mut_ptr(),
        b".\x00" as *const u8 as *const libc::c_char,
    ) == 0 as i32
        || namecmp(
            name.as_mut_ptr(),
            b"..\x00" as *const u8 as *const libc::c_char,
        ) == 0 as i32)
    {
        ip = dirlookup(dp, name.as_mut_ptr(), &mut off);
        if !ip.is_null() {
            ilock(ip);
            if ((*ip).nlink as i32) < 1 as i32 {
                panic(
                    b"unlink: nlink < 1\x00" as *const u8 as *const libc::c_char
                        as *mut libc::c_char,
                );
            }
            if (*ip).type_0 as i32 == T_DIR && isdirempty(ip) == 0 {
                iunlockput(ip);
            } else {
                memset(
                    &mut de as *mut dirent as *mut libc::c_void,
                    0 as i32,
                    ::core::mem::size_of::<dirent>() as u64 as u32,
                );
                if writei(
                    dp,
                    0,
                    &mut de as *mut dirent as u64,
                    off,
                    ::core::mem::size_of::<dirent>() as u64 as u32,
                ) as u64
                    != ::core::mem::size_of::<dirent>() as u64
                {
                    panic(
                        b"unlink: writei\x00" as *const u8 as *const libc::c_char
                            as *mut libc::c_char,
                    );
                }
                if (*ip).type_0 as i32 == T_DIR {
                    (*dp).nlink -= 1;
                    iupdate(dp);
                }
                iunlockput(dp);
                (*ip).nlink -= 1;
                iupdate(ip);
                iunlockput(ip);
                end_op();
                return 0;
            }
        }
    }
    iunlockput(dp);
    end_op();
    -(1 as i32) as u64
}
unsafe extern "C" fn create(
    mut path: *mut libc::c_char,
    mut type_0: i16,
    mut major: i16,
    mut minor: i16,
) -> *mut inode {
    let mut ip: *mut inode = ptr::null_mut();
    let mut dp: *mut inode = ptr::null_mut();
    let mut name: [libc::c_char; 14] = [0; 14];
    dp = nameiparent(path, name.as_mut_ptr());
    if dp.is_null() {
        return ptr::null_mut();
    }
    ilock(dp);
    ip = dirlookup(dp, name.as_mut_ptr(), ptr::null_mut());
    if !ip.is_null() {
        iunlockput(dp);
        ilock(ip);
        if type_0 as i32 == T_FILE
            && ((*ip).type_0 as i32 == T_FILE || (*ip).type_0 as i32 == T_DEVICE)
        {
            return ip;
        }
        iunlockput(ip);
        return ptr::null_mut();
    }
    ip = ialloc((*dp).dev, type_0);
    if ip.is_null() {
        panic(b"create: ialloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ilock(ip);
    (*ip).major = major;
    (*ip).minor = minor;
    (*ip).nlink = 1 as i16;
    iupdate(ip);
    if type_0 as i32 == T_DIR {
        // Create . and .. entries.
        (*dp).nlink += 1; // for ".."
        iupdate(dp);
        // No ip->nlink++ for ".": avoid cyclic ref count.
        if dirlink(
            ip,
            b".\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            (*ip).inum,
        ) < 0 as i32
            || dirlink(
                ip,
                b"..\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                (*dp).inum,
            ) < 0 as i32
        {
            panic(b"create dots\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
            // user pointer to array of two integers
        }
    }
    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 as i32 {
        panic(b"create: dirlink\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    iunlockput(dp);
    ip
}
#[no_mangle]
pub unsafe extern "C" fn sys_open() -> u64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut fd: i32 = 0;
    let mut omode: i32 = 0;
    let mut f: *mut File = ptr::null_mut();
    let mut ip: *mut inode = ptr::null_mut();
    let mut n: i32 = 0;
    n = argstr(0 as i32, path.as_mut_ptr(), MAXPATH);
    if n < 0 as i32 || argint(1 as i32, &mut omode) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    begin_op();
    if omode & O_CREATE != 0 {
        ip = create(
            path.as_mut_ptr(),
            T_FILE as i16,
            0 as i32 as i16,
            0 as i32 as i16,
        );
        if ip.is_null() {
            end_op();
            return -(1 as i32) as u64;
        }
    } else {
        ip = namei(path.as_mut_ptr());
        if ip.is_null() {
            end_op();
            return -(1 as i32) as u64;
        }
        ilock(ip);
        if (*ip).type_0 as i32 == T_DIR && omode != O_RDONLY {
            iunlockput(ip);
            end_op();
            return -(1 as i32) as u64;
        }
    }
    if (*ip).type_0 as i32 == T_DEVICE
        && (((*ip).major as i32) < 0 as i32 || (*ip).major as i32 >= NDEV)
    {
        iunlockput(ip);
        end_op();
        return -(1 as i32) as u64;
    }
    f = filealloc();
    if f.is_null() || {
        fd = fdalloc(f);
        (fd) < 0 as i32
    } {
        if !f.is_null() {
            fileclose(f);
        }
        iunlockput(ip);
        end_op();
        return -(1 as i32) as u64;
    }
    if (*ip).type_0 as i32 == T_DEVICE {
        (*f).type_0 = FD_DEVICE;
        (*f).major = (*ip).major
    } else {
        (*f).type_0 = FD_INODE;
        (*f).off = 0 as i32 as u32
    }
    (*f).ip = ip;
    (*f).readable = (omode & O_WRONLY == 0) as i32 as libc::c_char;
    (*f).writable = (omode & O_WRONLY != 0 || omode & O_RDWR != 0) as i32 as libc::c_char;
    iunlock(ip);
    end_op();
    fd as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_mkdir() -> u64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut ip: *mut inode = ptr::null_mut();
    begin_op();
    if argstr(0 as i32, path.as_mut_ptr(), MAXPATH) < 0 as i32 || {
        ip = create(
            path.as_mut_ptr(),
            T_DIR as i16,
            0 as i32 as i16,
            0 as i32 as i16,
        );
        ip.is_null()
    } {
        end_op();
        return -(1 as i32) as u64;
    }
    iunlockput(ip);
    end_op();
    0
}
#[no_mangle]
pub unsafe extern "C" fn sys_mknod() -> u64 {
    let mut ip: *mut inode = ptr::null_mut();
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut major: i32 = 0;
    let mut minor: i32 = 0;
    begin_op();
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 as i32
        || argint(1, &mut major) < 0 as i32
        || argint(2, &mut minor) < 0 as i32
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
        return -(1 as i32) as u64;
    }
    iunlockput(ip);
    end_op();
    0 as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_chdir() -> u64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut ip: *mut inode = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    begin_op();
    if argstr(0 as i32, path.as_mut_ptr(), MAXPATH) < 0 as i32 || {
        ip = namei(path.as_mut_ptr());
        ip.is_null()
    } {
        end_op();
        return -(1 as i32) as u64;
    }
    ilock(ip);
    if (*ip).type_0 as i32 != T_DIR {
        iunlockput(ip);
        end_op();
        return -(1 as i32) as u64;
    }
    iunlock(ip);
    iput((*p).cwd);
    end_op();
    (*p).cwd = ip;
    0 as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_exec() -> u64 {
    let mut ret: i32 = 0;
    let mut current_block: u64;
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut argv: [*mut libc::c_char; 32] = [ptr::null_mut(); 32];
    let mut i: i32 = 0;
    let mut uargv: u64 = 0;
    let mut uarg: u64 = 0;
    if argstr(0, path.as_mut_ptr(), MAXPATH) < 0 as i32 || argaddr(1 as i32, &mut uargv) < 0 as i32
    {
        return -(1 as i32) as u64;
    }
    memset(
        argv.as_mut_ptr() as *mut libc::c_void,
        0,
        ::core::mem::size_of::<[*mut libc::c_char; 32]>() as u64 as u32,
    );
    i = 0 as i32;
    loop {
        if i as u64
            >= (::core::mem::size_of::<[*mut libc::c_char; 32]>() as u64)
                .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as u64)
        {
            current_block = 12646643519710607562;
            break;
        }
        if fetchaddr(
            uargv.wrapping_add((::core::mem::size_of::<u64>() as u64).wrapping_mul(i as u64)),
            &mut uarg as *mut u64,
        ) < 0 as i32
        {
            current_block = 12646643519710607562;
            break;
        }
        if uarg == 0 as i32 as u64 {
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
            if fetchstr(uarg, argv[i as usize], PGSIZE) < 0 as i32 {
                current_block = 12646643519710607562;
                break;
            }
            i += 1
        }
    }
    match current_block {
        12646643519710607562 => {
            i = 0 as i32;
            while (i as u64)
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>() as u64)
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as u64)
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            -(1 as i32) as u64
        }
        _ => {
            ret = exec(path.as_mut_ptr(), argv.as_mut_ptr());
            i = 0 as i32;
            while (i as u64)
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>() as u64)
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as u64)
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            ret as u64
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sys_pipe() -> u64 {
    let mut fdarray: u64 = 0;
    let mut rf: *mut File = ptr::null_mut();
    let mut wf: *mut File = ptr::null_mut();
    let mut fd0: i32 = 0;
    let mut fd1: i32 = 0;
    let mut p: *mut proc_0 = myproc();
    if argaddr(0 as i32, &mut fdarray) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    if pipealloc(&mut rf, &mut wf) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    fd0 = -(1 as i32);
    fd0 = fdalloc(rf);
    if fd0 < 0 as i32 || {
        fd1 = fdalloc(wf);
        (fd1) < 0 as i32
    } {
        if fd0 >= 0 as i32 {
            (*p).ofile[fd0 as usize] = ptr::null_mut()
        }
        fileclose(rf);
        fileclose(wf);
        return -(1 as i32) as u64;
    }
    if copyout(
        (*p).pagetable,
        fdarray,
        &mut fd0 as *mut i32 as *mut libc::c_char,
        ::core::mem::size_of::<i32>() as u64,
    ) < 0
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(::core::mem::size_of::<i32>() as u64),
            &mut fd1 as *mut i32 as *mut libc::c_char,
            ::core::mem::size_of::<i32>() as u64,
        ) < 0
    {
        (*p).ofile[fd0 as usize] = ptr::null_mut();
        (*p).ofile[fd1 as usize] = ptr::null_mut();
        fileclose(rf);
        fileclose(wf);
        return -(1 as i32) as u64;
    }
    0
}
