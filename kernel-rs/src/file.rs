use crate::fs::{ilock, iput, iunlock, readi, stati, writei};
use crate::libc;
use crate::pipe::{pipeclose, piperead, pipewrite, Pipe};
use crate::proc::{myproc, proc_0};
use crate::sleeplock::Sleeplock;
use crate::spinlock::{acquire, initlock, release, Spinlock};
use crate::stat::Stat;
use core::ptr;
extern "C" {
    pub type pipe;
    #[no_mangle]
    fn begin_op();
    #[no_mangle]
    fn end_op();
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct File {
    pub type_0: C2RustUnnamed,
    pub ref_0: libc::c_int,
    pub readable: libc::c_char,
    pub writable: libc::c_char,
    pub pipe: *mut Pipe,
    pub ip: *mut inode,
    pub off: uint,
    pub major: libc::c_short,
}
// FD_DEVICE
// in-memory copy of an inode
#[derive(Copy, Clone)]
#[repr(C)]
pub struct inode {
    pub dev: uint,
    pub inum: uint,
    pub ref_0: libc::c_int,
    pub lock: Sleeplock,
    pub valid: libc::c_int,
    pub type_0: libc::c_short,
    pub major: libc::c_short,
    pub minor: libc::c_short,
    pub nlink: libc::c_short,
    pub size: uint,
    pub addrs: [uint; 13],
}

pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub lock: Spinlock,
    pub file: [File; 100],
}
// map major device number to device functions.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct devsw {
    pub read:
        Option<unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int>,
    pub write:
        Option<unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int>,
}
// maximum number of processes
// maximum number of CPUs
// open files per process
pub const NFILE: libc::c_int = 100 as libc::c_int;
// open files per system
// maximum number of active i-nodes
pub const NDEV: libc::c_int = 10 as libc::c_int;
// maximum major device number
// device number of file system root disk
// max exec arguments
pub const MAXOPBLOCKS: libc::c_int = 10 as libc::c_int;
// On-disk file system format.
// Both the kernel and user programs use this header file.
// root i-number
pub const BSIZE: libc::c_int = 1024 as libc::c_int;
// //
// Support functions for system calls that involve file descriptors.
//
#[no_mangle]
pub static mut devsw: [devsw; 10] = [devsw {
    read: None,
    write: None,
}; 10];
#[no_mangle]
pub static mut ftable: C2RustUnnamed_0 = C2RustUnnamed_0 {
    lock: Spinlock {
        locked: 0,
        name: ptr::null_mut(),
        cpu: ptr::null_mut(),
    },
    file: [File {
        type_0: FD_NONE,
        ref_0: 0,
        readable: 0,
        writable: 0,
        pipe: 0 as *const Pipe as *mut Pipe,
        ip: 0 as *const inode as *mut inode,
        off: 0,
        major: 0,
    }; 100],
};
#[no_mangle]
pub unsafe extern "C" fn fileinit() {
    initlock(
        &mut ftable.lock,
        b"ftable\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
}
// file.c
// Allocate a file structure.
#[no_mangle]
pub unsafe extern "C" fn filealloc() -> *mut File {
    let mut f: *mut File = ptr::null_mut();
    acquire(&mut ftable.lock);
    f = ftable.file.as_mut_ptr();
    while f < ftable.file.as_mut_ptr().offset(NFILE as isize) {
        if (*f).ref_0 == 0 as libc::c_int {
            (*f).ref_0 = 1 as libc::c_int;
            release(&mut ftable.lock);
            return f;
        }
        f = f.offset(1)
    }
    release(&mut ftable.lock);
    ptr::null_mut()
}
// Increment ref count for file f.
#[no_mangle]
pub unsafe extern "C" fn filedup(mut f: *mut File) -> *mut File {
    acquire(&mut ftable.lock);
    if (*f).ref_0 < 1 as libc::c_int {
        panic(b"filedup\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*f).ref_0 += 1;
    release(&mut ftable.lock);
    f
}
// Close file f.  (Decrement ref count, close when reaches 0.)
#[no_mangle]
pub unsafe extern "C" fn fileclose(mut f: *mut File) {
    let mut ff: File = File {
        type_0: FD_NONE,
        ref_0: 0,
        readable: 0,
        writable: 0,
        pipe: 0 as *const Pipe as *mut Pipe,
        ip: 0 as *const inode as *mut inode,
        off: 0,
        major: 0,
    };
    acquire(&mut ftable.lock);
    if (*f).ref_0 < 1 as libc::c_int {
        panic(b"fileclose\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*f).ref_0 -= 1;
    if (*f).ref_0 > 0 as libc::c_int {
        release(&mut ftable.lock);
        return;
    }
    ff = *f;
    (*f).ref_0 = 0 as libc::c_int;
    (*f).type_0 = FD_NONE;
    release(&mut ftable.lock);
    if ff.type_0 as libc::c_uint == FD_PIPE as libc::c_int as libc::c_uint {
        pipeclose(ff.pipe, ff.writable as libc::c_int);
    } else if ff.type_0 as libc::c_uint == FD_INODE as libc::c_int as libc::c_uint
        || ff.type_0 as libc::c_uint == FD_DEVICE as libc::c_int as libc::c_uint
    {
        begin_op();
        iput(ff.ip);
        end_op();
    };
}
// Get metadata about file f.
// addr is a user virtual address, pointing to a struct stat.
#[no_mangle]
pub unsafe extern "C" fn filestat(mut f: *mut File, mut addr: uint64) -> libc::c_int {
    let mut p: *mut proc_0 = myproc();
    let mut st: Stat = Stat {
        dev: 0,
        ino: 0,
        type_0: 0,
        nlink: 0,
        size: 0,
    };
    if (*f).type_0 as libc::c_uint == FD_INODE as libc::c_int as libc::c_uint
        || (*f).type_0 as libc::c_uint == FD_DEVICE as libc::c_int as libc::c_uint
    {
        ilock((*f).ip);
        stati((*f).ip, &mut st);
        iunlock((*f).ip);
        if copyout(
            (*p).pagetable,
            addr,
            &mut st as *mut Stat as *mut libc::c_char,
            ::core::mem::size_of::<Stat>() as libc::c_ulong,
        ) < 0 as libc::c_int
        {
            return -(1 as libc::c_int);
        }
        return 0 as libc::c_int;
    }
    -(1 as libc::c_int)
}
// Read from file f.
// addr is a user virtual address.
#[no_mangle]
pub unsafe extern "C" fn fileread(
    mut f: *mut File,
    mut addr: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut r: libc::c_int = 0 as libc::c_int;
    if (*f).readable as libc::c_int == 0 as libc::c_int {
        return -(1 as libc::c_int);
    }
    if (*f).type_0 as libc::c_uint == FD_PIPE as libc::c_int as libc::c_uint {
        r = piperead((*f).pipe, addr, n)
    } else if (*f).type_0 as libc::c_uint == FD_DEVICE as libc::c_int as libc::c_uint {
        if ((*f).major as libc::c_int) < 0 as libc::c_int
            || (*f).major as libc::c_int >= NDEV
            || devsw[(*f).major as usize].read.is_none()
        {
            return -(1 as libc::c_int);
        }
        r = devsw[(*f).major as usize]
            .read
            .expect("non-null function pointer")(1 as libc::c_int, addr, n)
    } else if (*f).type_0 as libc::c_uint == FD_INODE as libc::c_int as libc::c_uint {
        ilock((*f).ip);
        r = readi((*f).ip, 1 as libc::c_int, addr, (*f).off, n as uint);
        if r > 0 as libc::c_int {
            (*f).off = ((*f).off as libc::c_uint).wrapping_add(r as libc::c_uint) as uint as uint
        }
        iunlock((*f).ip);
    } else {
        panic(b"fileread\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    r
}
// Write to file f.
// addr is a user virtual address.
#[no_mangle]
pub unsafe extern "C" fn filewrite(
    mut f: *mut File,
    mut addr: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut r: libc::c_int = 0;
    let mut ret: libc::c_int = 0 as libc::c_int;
    if (*f).writable as libc::c_int == 0 as libc::c_int {
        return -(1 as libc::c_int);
    }
    if (*f).type_0 as libc::c_uint == FD_PIPE as libc::c_int as libc::c_uint {
        ret = pipewrite((*f).pipe, addr, n)
    } else if (*f).type_0 as libc::c_uint == FD_DEVICE as libc::c_int as libc::c_uint {
        if ((*f).major as libc::c_int) < 0 as libc::c_int
            || (*f).major as libc::c_int >= NDEV
            || devsw[(*f).major as usize].write.is_none()
        {
            return -(1 as libc::c_int);
        }
        ret = devsw[(*f).major as usize]
            .write
            .expect("non-null function pointer")(1 as libc::c_int, addr, n)
    } else if (*f).type_0 as libc::c_uint == FD_INODE as libc::c_int as libc::c_uint {
        // write a few blocks at a time to avoid exceeding
        // the maximum log transaction size, including
        // i-node, indirect block, allocation blocks,
        // and 2 blocks of slop for non-aligned writes.
        // this really belongs lower down, since writei()
        // might be writing a device like the console.
        let mut max: libc::c_int =
            (MAXOPBLOCKS - 1 as libc::c_int - 1 as libc::c_int - 2 as libc::c_int)
                / 2 as libc::c_int
                * BSIZE;
        let mut i: libc::c_int = 0 as libc::c_int;
        while i < n {
            let mut n1: libc::c_int = n - i;
            if n1 > max {
                n1 = max
            }
            begin_op();
            ilock((*f).ip);
            r = writei(
                (*f).ip,
                1 as libc::c_int,
                addr.wrapping_add(i as libc::c_ulong),
                (*f).off,
                n1 as uint,
            );
            if r > 0 as libc::c_int {
                (*f).off =
                    ((*f).off as libc::c_uint).wrapping_add(r as libc::c_uint) as uint as uint
            }
            iunlock((*f).ip);
            end_op();
            if r < 0 as libc::c_int {
                break;
            }
            if r != n1 {
                panic(
                    b"short filewrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                );
            }
            i += r
        }
        ret = if i == n { n } else { -(1 as libc::c_int) }
    } else {
        panic(b"filewrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ret
}
