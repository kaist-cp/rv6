use crate::{ libc, proc::proc_0, file::{ File, inode }, fs::dirent};
use core::ptr;
extern "C" {
    // pub type pipe;
    // exec.c
    #[no_mangle]
    fn exec(_: *mut libc::c_char, _: *mut *mut libc::c_char) -> libc::c_int;
    // file.c
    #[no_mangle]
    fn filealloc() -> *mut File;
    #[no_mangle]
    fn fileclose(_: *mut File);
    #[no_mangle]
    fn filedup(_: *mut File) -> *mut File;
    #[no_mangle]
    fn fileread(_: *mut File, _: uint64, n: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn filestat(_: *mut File, addr: uint64) -> libc::c_int;
    #[no_mangle]
    fn filewrite(_: *mut File, _: uint64, n: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn dirlink(_: *mut inode, _: *mut libc::c_char, _: uint) -> libc::c_int;
    #[no_mangle]
    fn dirlookup(_: *mut inode, _: *mut libc::c_char, _: *mut uint) -> *mut inode;
    #[no_mangle]
    fn ialloc(_: uint, _: libc::c_short) -> *mut inode;
    #[no_mangle]
    fn ilock(_: *mut inode);
    #[no_mangle]
    fn iput(_: *mut inode);
    #[no_mangle]
    fn iunlock(_: *mut inode);
    #[no_mangle]
    fn iunlockput(_: *mut inode);
    #[no_mangle]
    fn iupdate(_: *mut inode);
    #[no_mangle]
    fn namecmp(_: *const libc::c_char, _: *const libc::c_char) -> libc::c_int;
    #[no_mangle]
    fn namei(_: *mut libc::c_char) -> *mut inode;
    #[no_mangle]
    fn nameiparent(_: *mut libc::c_char, _: *mut libc::c_char) -> *mut inode;
    #[no_mangle]
    fn readi(_: *mut inode, _: libc::c_int, _: uint64, _: uint, _: uint) -> libc::c_int;
    #[no_mangle]
    fn writei(_: *mut inode, _: libc::c_int, _: uint64, _: uint, _: uint) -> libc::c_int;
    // kalloc.c
    #[no_mangle]
    fn kalloc() -> *mut libc::c_void;
    #[no_mangle]
    fn kfree(_: *mut libc::c_void);
    #[no_mangle]
    fn begin_op();
    #[no_mangle]
    fn end_op();
    // pipe.c
    #[no_mangle]
    fn pipealloc(_: *mut *mut File, _: *mut *mut File) -> libc::c_int;
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: libc::c_int, _: uint) -> *mut libc::c_void;
    // syscall.c
    #[no_mangle]
    fn argint(_: libc::c_int, _: *mut libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn argstr(_: libc::c_int, _: *mut libc::c_char, _: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn argaddr(_: libc::c_int, _: *mut uint64) -> libc::c_int;
    #[no_mangle]
    fn fetchstr(_: uint64, _: *mut libc::c_char, _: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn fetchaddr(_: uint64, _: *mut uint64) -> libc::c_int;
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
}
pub type uint = libc::c_uint;
pub type ushort = libc::c_ushort;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;
// // Saved registers for kernel context switches.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct context {
//     pub ra: uint64,
//     pub sp: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
// }
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct file {
//     pub type_0: C2RustUnnamed,
//     pub ref_0: libc::c_int,
//     pub readable: libc::c_char,
//     pub writable: libc::c_char,
//     pub pipe: *mut pipe,
//     pub ip: *mut inode,
//     pub off: uint,
//     pub major: libc::c_short,
// }
// // FD_DEVICE
// // in-memory copy of an inode
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct inode {
//     pub dev: uint,
//     pub inum: uint,
//     pub ref_0: libc::c_int,
//     pub lock: sleeplock,
//     pub valid: libc::c_int,
//     pub type_0: libc::c_short,
//     pub major: libc::c_short,
//     pub minor: libc::c_short,
//     pub nlink: libc::c_short,
//     pub size: uint,
//     pub addrs: [uint; 13],
// }
// // Long-term locks for processes
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct sleeplock {
//     pub locked: uint,
//     pub lk: spinlock,
//     pub name: *mut libc::c_char,
//     pub pid: libc::c_int,
// }
// // Mutual exclusion lock.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct spinlock {
//     pub locked: uint,
//     pub name: *mut libc::c_char,
//     pub cpu: *mut cpu,
// }
// // Per-CPU state.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct cpu {
//     pub proc_0: *mut proc_0,
//     pub scheduler: context,
//     pub noff: libc::c_int,
//     pub intena: libc::c_int,
// }
// // Per-process state
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct proc_0 {
//     pub lock: spinlock,
//     pub state: procstate,
//     pub parent: *mut proc_0,
//     pub chan: *mut libc::c_void,
//     pub killed: libc::c_int,
//     pub xstate: libc::c_int,
//     pub pid: libc::c_int,
//     pub kstack: uint64,
//     pub sz: uint64,
//     pub pagetable: pagetable_t,
//     pub tf: *mut trapframe,
//     pub context: context,
//     pub ofile: [*mut file; 16],
//     pub cwd: *mut inode,
//     pub name: [libc::c_char; 16],
// }
// // per-process data for the trap handling code in trampoline.S.
// // sits in a page by itself just under the trampoline page in the
// // user page table. not specially mapped in the kernel page table.
// // the sscratch register points here.
// // uservec in trampoline.S saves user registers in the trapframe,
// // then initializes registers from the trapframe's
// // kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// // usertrapret() and userret in trampoline.S set up
// // the trapframe's kernel_*, restore user registers from the
// // trapframe, switch to the user page table, and enter user space.
// // the trapframe includes callee-saved user registers like s0-s11 because the
// // return-to-user path via usertrapret() doesn't return through
// // the entire kernel call stack.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct trapframe {
//     pub kernel_satp: uint64,
//     pub kernel_sp: uint64,
//     pub kernel_trap: uint64,
//     pub epc: uint64,
//     pub kernel_hartid: uint64,
//     pub ra: uint64,
//     pub sp: uint64,
//     pub gp: uint64,
//     pub tp: uint64,
//     pub t0: uint64,
//     pub t1: uint64,
//     pub t2: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub a0: uint64,
//     pub a1: uint64,
//     pub a2: uint64,
//     pub a3: uint64,
//     pub a4: uint64,
//     pub a5: uint64,
//     pub a6: uint64,
//     pub a7: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
//     pub t3: uint64,
//     pub t4: uint64,
//     pub t5: uint64,
//     pub t6: uint64,
// }
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;
// Data block addresses
// Inodes per block.
// Block containing inode i
// Bitmap bits per block
// Block of free map containing bit for block b
// Directory is a file containing a sequence of dirent structures.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct dirent {
//     pub inum: ushort,
//     pub name: [libc::c_char; 14],
// }
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// maximum number of processes
// maximum number of CPUs
pub const NOFILE: libc::c_int = 16 as libc::c_int;
// open files per process
// open files per system
// maximum number of active i-nodes
pub const NDEV: libc::c_int = 10 as libc::c_int;
// maximum major device number
// device number of file system root disk
// max exec arguments
// max # of blocks any FS op writes
// max data blocks in on-disk log
// size of disk block cache
// size of file system in blocks
pub const MAXPATH: libc::c_int = 128 as libc::c_int;
pub const T_DIR: libc::c_int = 1 as libc::c_int;
// Directory
pub const T_FILE: libc::c_int = 2 as libc::c_int;
// File
pub const T_DEVICE: libc::c_int = 3 as libc::c_int;
pub const O_RDONLY: libc::c_int = 0 as libc::c_int;
pub const O_WRONLY: libc::c_int = 0x1 as libc::c_int;
pub const O_RDWR: libc::c_int = 0x2 as libc::c_int;
pub const O_CREATE: libc::c_int = 0x200 as libc::c_int;
//
// File-system system calls.
// Mostly argument checking, since we don't trust
// user code, and calls into file.c and fs.c.
//
// Fetch the nth word-sized system call argument as a file descriptor
// and return both the descriptor and the corresponding struct file.
unsafe extern "C" fn argfd(
    mut n: libc::c_int,
    mut pfd: *mut libc::c_int,
    mut pf: *mut *mut File,
) -> libc::c_int {
    let mut fd: libc::c_int = 0;
    let mut f: *mut File = ptr::null_mut();
    if argint(n, &mut fd) < 0 as libc::c_int {
        return -(1 as libc::c_int);
    }
    if fd < 0 as libc::c_int || fd >= NOFILE || {
        f = (*myproc()).ofile[fd as usize];
        f.is_null()
    } {
        return -(1 as libc::c_int);
    }
    if !pfd.is_null() {
        *pfd = fd
    }
    if !pf.is_null() {
        *pf = f
    }
    0 as libc::c_int
}
// Allocate a file descriptor for the given file.
// Takes over file reference from caller on success.
unsafe extern "C" fn fdalloc(mut f: *mut File) -> libc::c_int {
    let mut fd: libc::c_int = 0; // user pointer to struct stat
    let mut p: *mut proc_0 = myproc();
    fd = 0 as libc::c_int;
    while fd < NOFILE {
        if (*p).ofile[fd as usize].is_null() {
            (*p).ofile[fd as usize] = f;
            return fd;
        }
        fd += 1
    }
    -(1 as libc::c_int)
}
#[no_mangle]
pub unsafe extern "C" fn sys_dup() -> uint64 {
    let mut f: *mut File = ptr::null_mut();
    let mut fd: libc::c_int = 0;
    if argfd(0 as libc::c_int, ptr::null_mut(), &mut f) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    fd = fdalloc(f);
    if fd < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    filedup(f);
    fd as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_read() -> uint64 {
    let mut f: *mut File = ptr::null_mut();
    let mut n: libc::c_int = 0;
    let mut p: uint64 = 0;
    if argfd(0 as libc::c_int, ptr::null_mut(), &mut f) < 0 as libc::c_int
        || argint(2 as libc::c_int, &mut n) < 0 as libc::c_int
        || argaddr(1 as libc::c_int, &mut p) < 0 as libc::c_int
    {
        return -(1 as libc::c_int) as uint64;
    }
    fileread(f, p, n) as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_write() -> uint64 {
    let mut f: *mut File = ptr::null_mut();
    let mut n: libc::c_int = 0;
    let mut p: uint64 = 0;
    if argfd(0 as libc::c_int, ptr::null_mut(), &mut f) < 0 as libc::c_int
        || argint(2 as libc::c_int, &mut n) < 0 as libc::c_int
        || argaddr(1 as libc::c_int, &mut p) < 0 as libc::c_int
    {
        return -(1 as libc::c_int) as uint64;
    }
    filewrite(f, p, n) as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_close() -> uint64 {
    let mut fd: libc::c_int = 0;
    let mut f: *mut File = ptr::null_mut();
    if argfd(0 as libc::c_int, &mut fd, &mut f) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    let fresh0 = &mut (*myproc()).ofile[fd as usize];
    *fresh0 = ptr::null_mut();
    fileclose(f);
    0 as libc::c_int as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_fstat() -> uint64 {
    let mut f: *mut File = ptr::null_mut();
    let mut st: uint64 = 0;
    if argfd(0 as libc::c_int, ptr::null_mut(), &mut f) < 0 as libc::c_int
        || argaddr(1 as libc::c_int, &mut st) < 0 as libc::c_int
    {
        return -(1 as libc::c_int) as uint64;
    }
    filestat(f, st) as uint64
}
// Create the path new as a link to the same inode as old.
#[no_mangle]
pub unsafe extern "C" fn sys_link() -> uint64 {
    let mut name: [libc::c_char; 14] = [0; 14];
    let mut new: [libc::c_char; 128] = [0; 128];
    let mut old: [libc::c_char; 128] = [0; 128];
    let mut dp: *mut inode = ptr::null_mut();
    let mut ip: *mut inode = ptr::null_mut();
    if argstr(0 as libc::c_int, old.as_mut_ptr(), MAXPATH) < 0 as libc::c_int
        || argstr(1 as libc::c_int, new.as_mut_ptr(), MAXPATH) < 0 as libc::c_int
    {
        return -(1 as libc::c_int) as uint64;
    }
    begin_op();
    ip = namei(old.as_mut_ptr());
    if ip.is_null() {
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    ilock(ip);
    if (*ip).type_0 as libc::c_int == T_DIR {
        iunlockput(ip);
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    (*ip).nlink += 1;
    iupdate(ip);
    iunlock(ip);
    dp = nameiparent(new.as_mut_ptr(), name.as_mut_ptr());
    if !dp.is_null() {
        ilock(dp);
        if (*dp).dev != (*ip).dev || dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 as libc::c_int {
            iunlockput(dp);
        } else {
            iunlockput(dp);
            iput(ip);
            end_op();
            return 0 as libc::c_int as uint64;
        }
    }
    ilock(ip);
    (*ip).nlink -= 1;
    iupdate(ip);
    iunlockput(ip);
    end_op();
    -(1 as libc::c_int) as uint64
}
// Is the directory dp empty except for "." and ".." ?
unsafe extern "C" fn isdirempty(mut dp: *mut inode) -> libc::c_int {
    let mut off: libc::c_int = 0;
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    off = (2 as libc::c_int as libc::c_ulong)
        .wrapping_mul(::core::mem::size_of::<dirent>() as libc::c_ulong) as libc::c_int;
    while (off as libc::c_uint) < (*dp).size {
        if readi(
            dp,
            0 as libc::c_int,
            &mut de as *mut dirent as uint64,
            off as uint,
            ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
        ) as libc::c_ulong
            != ::core::mem::size_of::<dirent>() as libc::c_ulong
        {
            panic(
                b"isdirempty: readi\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
        }
        if de.inum as libc::c_int != 0 as libc::c_int {
            return 0 as libc::c_int;
        }
        off = (off as libc::c_ulong).wrapping_add(::core::mem::size_of::<dirent>() as libc::c_ulong)
            as libc::c_int as libc::c_int
    }
    1 as libc::c_int
}
#[no_mangle]
pub unsafe extern "C" fn sys_unlink() -> uint64 {
    let mut ip: *mut inode = ptr::null_mut();
    let mut dp: *mut inode = ptr::null_mut();
    let mut de: dirent = dirent {
        inum: 0,
        name: [0; 14],
    };
    let mut name: [libc::c_char; 14] = [0; 14];
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut off: uint = 0;
    if argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    begin_op();
    dp = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    ilock(dp);
    // Cannot unlink "." or "..".
    if !(namecmp(
        name.as_mut_ptr(),
        b".\x00" as *const u8 as *const libc::c_char,
    ) == 0 as libc::c_int
        || namecmp(
            name.as_mut_ptr(),
            b"..\x00" as *const u8 as *const libc::c_char,
        ) == 0 as libc::c_int)
    {
        ip = dirlookup(dp, name.as_mut_ptr(), &mut off);
        if !ip.is_null() {
            ilock(ip);
            if ((*ip).nlink as libc::c_int) < 1 as libc::c_int {
                panic(
                    b"unlink: nlink < 1\x00" as *const u8 as *const libc::c_char
                        as *mut libc::c_char,
                );
            }
            if (*ip).type_0 as libc::c_int == T_DIR && isdirempty(ip) == 0 {
                iunlockput(ip);
            } else {
                memset(
                    &mut de as *mut dirent as *mut libc::c_void,
                    0 as libc::c_int,
                    ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
                );
                if writei(
                    dp,
                    0 as libc::c_int,
                    &mut de as *mut dirent as uint64,
                    off,
                    ::core::mem::size_of::<dirent>() as libc::c_ulong as uint,
                ) as libc::c_ulong
                    != ::core::mem::size_of::<dirent>() as libc::c_ulong
                {
                    panic(
                        b"unlink: writei\x00" as *const u8 as *const libc::c_char
                            as *mut libc::c_char,
                    );
                }
                if (*ip).type_0 as libc::c_int == T_DIR {
                    (*dp).nlink -= 1;
                    iupdate(dp);
                }
                iunlockput(dp);
                (*ip).nlink -= 1;
                iupdate(ip);
                iunlockput(ip);
                end_op();
                return 0 as libc::c_int as uint64;
            }
        }
    }
    iunlockput(dp);
    end_op();
    -(1 as libc::c_int) as uint64
}
unsafe extern "C" fn create(
    mut path: *mut libc::c_char,
    mut type_0: libc::c_short,
    mut major: libc::c_short,
    mut minor: libc::c_short,
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
        if type_0 as libc::c_int == T_FILE
            && ((*ip).type_0 as libc::c_int == T_FILE || (*ip).type_0 as libc::c_int == T_DEVICE)
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
    (*ip).nlink = 1 as libc::c_int as libc::c_short;
    iupdate(ip);
    if type_0 as libc::c_int == T_DIR {
        // Create . and .. entries.
        (*dp).nlink += 1; // for ".."
        iupdate(dp);
        // No ip->nlink++ for ".": avoid cyclic ref count.
        if dirlink(
            ip,
            b".\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            (*ip).inum,
        ) < 0 as libc::c_int
            || dirlink(
                ip,
                b"..\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                (*dp).inum,
            ) < 0 as libc::c_int
        {
            panic(b"create dots\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
            // user pointer to array of two integers
        }
    }
    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 as libc::c_int {
        panic(b"create: dirlink\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    iunlockput(dp);
    ip
}
#[no_mangle]
pub unsafe extern "C" fn sys_open() -> uint64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut fd: libc::c_int = 0;
    let mut omode: libc::c_int = 0;
    let mut f: *mut File = ptr::null_mut();
    let mut ip: *mut inode = ptr::null_mut();
    let mut n: libc::c_int = 0;
    n = argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH);
    if n < 0 as libc::c_int || argint(1 as libc::c_int, &mut omode) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    begin_op();
    if omode & O_CREATE != 0 {
        ip = create(
            path.as_mut_ptr(),
            T_FILE as libc::c_short,
            0 as libc::c_int as libc::c_short,
            0 as libc::c_int as libc::c_short,
        );
        if ip.is_null() {
            end_op();
            return -(1 as libc::c_int) as uint64;
        }
    } else {
        ip = namei(path.as_mut_ptr());
        if ip.is_null() {
            end_op();
            return -(1 as libc::c_int) as uint64;
        }
        ilock(ip);
        if (*ip).type_0 as libc::c_int == T_DIR && omode != O_RDONLY {
            iunlockput(ip);
            end_op();
            return -(1 as libc::c_int) as uint64;
        }
    }
    if (*ip).type_0 as libc::c_int == T_DEVICE
        && (((*ip).major as libc::c_int) < 0 as libc::c_int || (*ip).major as libc::c_int >= NDEV)
    {
        iunlockput(ip);
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    f = filealloc();
    if f.is_null() || {
        fd = fdalloc(f);
        (fd) < 0 as libc::c_int
    } {
        if !f.is_null() {
            fileclose(f);
        }
        iunlockput(ip);
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    if (*ip).type_0 as libc::c_int == T_DEVICE {
        (*f).type_0 = FD_DEVICE;
        (*f).major = (*ip).major
    } else {
        (*f).type_0 = FD_INODE;
        (*f).off = 0 as libc::c_int as uint
    }
    (*f).ip = ip;
    (*f).readable = (omode & O_WRONLY == 0) as libc::c_int as libc::c_char;
    (*f).writable = (omode & O_WRONLY != 0 || omode & O_RDWR != 0) as libc::c_int as libc::c_char;
    iunlock(ip);
    end_op();
    fd as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_mkdir() -> uint64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut ip: *mut inode = ptr::null_mut();
    begin_op();
    if argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH) < 0 as libc::c_int || {
        ip = create(
            path.as_mut_ptr(),
            T_DIR as libc::c_short,
            0 as libc::c_int as libc::c_short,
            0 as libc::c_int as libc::c_short,
        );
        ip.is_null()
    } {
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    iunlockput(ip);
    end_op();
    0 as libc::c_int as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_mknod() -> uint64 {
    let mut ip: *mut inode = ptr::null_mut();
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut major: libc::c_int = 0;
    let mut minor: libc::c_int = 0;
    begin_op();
    if argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH) < 0 as libc::c_int
        || argint(1 as libc::c_int, &mut major) < 0 as libc::c_int
        || argint(2 as libc::c_int, &mut minor) < 0 as libc::c_int
        || {
            ip = create(
                path.as_mut_ptr(),
                T_DEVICE as libc::c_short,
                major as libc::c_short,
                minor as libc::c_short,
            );
            ip.is_null()
        }
    {
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    iunlockput(ip);
    end_op();
    0 as libc::c_int as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_chdir() -> uint64 {
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut ip: *mut inode = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    begin_op();
    if argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH) < 0 as libc::c_int || {
        ip = namei(path.as_mut_ptr());
        ip.is_null()
    } {
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    ilock(ip);
    if (*ip).type_0 as libc::c_int != T_DIR {
        iunlockput(ip);
        end_op();
        return -(1 as libc::c_int) as uint64;
    }
    iunlock(ip);
    iput((*p).cwd);
    end_op();
    (*p).cwd = ip;
    0 as libc::c_int as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_exec() -> uint64 {
    let mut ret: libc::c_int = 0;
    let mut current_block: u64;
    let mut path: [libc::c_char; 128] = [0; 128];
    let mut argv: [*mut libc::c_char; 32] = [ptr::null_mut(); 32];
    let mut i: libc::c_int = 0;
    let mut uargv: uint64 = 0;
    let mut uarg: uint64 = 0;
    if argstr(0 as libc::c_int, path.as_mut_ptr(), MAXPATH) < 0 as libc::c_int
        || argaddr(1 as libc::c_int, &mut uargv) < 0 as libc::c_int
    {
        return -(1 as libc::c_int) as uint64;
    }
    memset(
        argv.as_mut_ptr() as *mut libc::c_void,
        0 as libc::c_int,
        ::core::mem::size_of::<[*mut libc::c_char; 32]>() as libc::c_ulong as uint,
    );
    i = 0 as libc::c_int;
    loop {
        if i as libc::c_ulong
            >= (::core::mem::size_of::<[*mut libc::c_char; 32]>() as libc::c_ulong)
                .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as libc::c_ulong)
        {
            current_block = 12646643519710607562;
            break;
        }
        if fetchaddr(
            uargv.wrapping_add(
                (::core::mem::size_of::<uint64>() as libc::c_ulong)
                    .wrapping_mul(i as libc::c_ulong),
            ),
            &mut uarg as *mut uint64,
        ) < 0 as libc::c_int
        {
            current_block = 12646643519710607562;
            break;
        }
        if uarg == 0 as libc::c_int as libc::c_ulong {
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
            if fetchstr(uarg, argv[i as usize], PGSIZE) < 0 as libc::c_int {
                current_block = 12646643519710607562;
                break;
            }
            i += 1
        }
    }
    match current_block {
        12646643519710607562 => {
            i = 0 as libc::c_int;
            while (i as libc::c_ulong)
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>() as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as libc::c_ulong)
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            -(1 as libc::c_int) as uint64
        }
        _ => {
            ret = exec(path.as_mut_ptr(), argv.as_mut_ptr());
            i = 0 as libc::c_int;
            while (i as libc::c_ulong)
                < (::core::mem::size_of::<[*mut libc::c_char; 32]>() as libc::c_ulong)
                    .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as libc::c_ulong)
                && !argv[i as usize].is_null()
            {
                kfree(argv[i as usize] as *mut libc::c_void);
                i += 1
            }
            ret as uint64
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sys_pipe() -> uint64 {
    let mut fdarray: uint64 = 0;
    let mut rf: *mut File = ptr::null_mut();
    let mut wf: *mut File = ptr::null_mut();
    let mut fd0: libc::c_int = 0;
    let mut fd1: libc::c_int = 0;
    let mut p: *mut proc_0 = myproc();
    if argaddr(0 as libc::c_int, &mut fdarray) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    if pipealloc(&mut rf, &mut wf) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    fd0 = -(1 as libc::c_int);
    fd0 = fdalloc(rf);
    if fd0 < 0 as libc::c_int || {
        fd1 = fdalloc(wf);
        (fd1) < 0 as libc::c_int
    } {
        if fd0 >= 0 as libc::c_int {
            (*p).ofile[fd0 as usize] = ptr::null_mut()
        }
        fileclose(rf);
        fileclose(wf);
        return -(1 as libc::c_int) as uint64;
    }
    if copyout(
        (*p).pagetable,
        fdarray,
        &mut fd0 as *mut libc::c_int as *mut libc::c_char,
        ::core::mem::size_of::<libc::c_int>() as libc::c_ulong,
    ) < 0 as libc::c_int
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(::core::mem::size_of::<libc::c_int>() as libc::c_ulong),
            &mut fd1 as *mut libc::c_int as *mut libc::c_char,
            ::core::mem::size_of::<libc::c_int>() as libc::c_ulong,
        ) < 0 as libc::c_int
    {
        (*p).ofile[fd0 as usize] = ptr::null_mut();
        (*p).ofile[fd1 as usize] = ptr::null_mut();
        fileclose(rf);
        fileclose(wf);
        return -(1 as libc::c_int) as uint64;
    }
    0 as libc::c_int as uint64
}
