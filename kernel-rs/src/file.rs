use crate::libc;
use crate::{
    fs::{ilock, iput, iunlock, readi, stati, writei},
    log::{begin_op, end_op},
    pipe::{pipeclose, piperead, pipewrite, Pipe},
    printf::panic,
    proc::{myproc, proc_0},
    sleeplock::Sleeplock,
    spinlock::{acquire, initlock, release, Spinlock},
    stat::Stat,
    vm::copyout,
};
use core::ptr;
pub type uint = libc::c_uint;
pub type pagetable_t = *mut u64;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct File {
    pub type_0: C2RustUnnamed,
    pub ref_0: i32,
    pub readable: libc::c_char,
    pub writable: libc::c_char,
    pub pipe: *mut Pipe,
    pub ip: *mut inode,
    pub off: u32,
    pub major: i16,
}
/// FD_DEVICE
/// in-memory copy of an inode
#[derive(Copy, Clone)]
#[repr(C)]
pub struct inode {
    pub dev: u32,
    pub inum: u32,
    pub ref_0: i32,
    pub lock: Sleeplock,
    pub valid: i32,
    pub type_0: i16,
    pub major: i16,
    pub minor: i16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; 13],
}
pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Ftable {
    pub lock: Spinlock,
    pub file: [File; 100],
}
/// map major device number to device functions.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct devsw {
    pub read: Option<unsafe extern "C" fn(_: i32, _: u64, _: i32) -> i32>,
    pub write: Option<unsafe extern "C" fn(_: i32, _: u64, _: i32) -> i32>,
}
// maximum number of processes
// maximum number of CPUs
// open files per process
pub const NFILE: i32 = 100;
// open files per system
// maximum number of active i-nodes
pub const NDEV: i32 = 10;
// maximum major device number
// device number of file system root disk
// max exec arguments
pub const MAXOPBLOCKS: i32 = 10;
// On-disk file system format.
// Both the kernel and user programs use this header file.
// root i-number
pub const BSIZE: i32 = 1024;
// //
// Support functions for system calls that involve file descriptors.
//
#[no_mangle]
pub static mut devsw: [devsw; 10] = [devsw {
    read: None,
    write: None,
}; 10];
#[no_mangle]
pub static mut ftable: Ftable = Ftable {
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
/// Allocate a file structure.
#[no_mangle]
pub unsafe extern "C" fn filealloc() -> *mut File {
    let mut f: *mut File = ptr::null_mut();
    acquire(&mut ftable.lock);
    f = ftable.file.as_mut_ptr();
    while f < ftable.file.as_mut_ptr().offset(NFILE as isize) {
        if (*f).ref_0 == 0 as i32 {
            (*f).ref_0 = 1 as i32;
            release(&mut ftable.lock);
            return f;
        }
        f = f.offset(1)
    }
    release(&mut ftable.lock);
    ptr::null_mut()
}
/// Increment ref count for file f.
#[no_mangle]
pub unsafe extern "C" fn filedup(mut f: *mut File) -> *mut File {
    acquire(&mut ftable.lock);
    if (*f).ref_0 < 1 as i32 {
        panic(b"filedup\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*f).ref_0 += 1;
    release(&mut ftable.lock);
    f
}
/// Close file f.  (Decrement ref count, close when reaches 0.)
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
    if (*f).ref_0 < 1 as i32 {
        panic(b"fileclose\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*f).ref_0 -= 1;
    if (*f).ref_0 > 0 as i32 {
        release(&mut ftable.lock);
        return;
    }
    ff = *f;
    (*f).ref_0 = 0 as i32;
    (*f).type_0 = FD_NONE;
    release(&mut ftable.lock);
    if ff.type_0 as u32 == FD_PIPE as i32 as u32 {
        pipeclose(ff.pipe, ff.writable as i32);
    } else if ff.type_0 as u32 == FD_INODE as i32 as u32
        || ff.type_0 as u32 == FD_DEVICE as i32 as u32
    {
        begin_op();
        iput(ff.ip);
        end_op();
    };
}
/// Get metadata about file f.
/// addr is a user virtual address, pointing to a struct stat.
#[no_mangle]
pub unsafe extern "C" fn filestat(mut f: *mut File, mut addr: u64) -> i32 {
    let mut p: *mut proc_0 = myproc();
    let mut st: Stat = Stat {
        dev: 0,
        ino: 0,
        type_0: 0,
        nlink: 0,
        size: 0,
    };
    if (*f).type_0 as u32 == FD_INODE as i32 as u32 || (*f).type_0 as u32 == FD_DEVICE as i32 as u32
    {
        ilock((*f).ip);
        stati((*f).ip, &mut st);
        iunlock((*f).ip);
        if copyout(
            (*p).pagetable,
            addr,
            &mut st as *mut Stat as *mut libc::c_char,
            ::core::mem::size_of::<Stat>() as u64,
        ) < 0 as i32
        {
            return -(1 as i32);
        }
        return 0 as i32;
    }
    -(1 as i32)
}
/// Read from file f.
/// addr is a user virtual address.
#[no_mangle]
pub unsafe extern "C" fn fileread(mut f: *mut File, mut addr: u64, mut n: i32) -> i32 {
    let mut r: i32 = 0;
    if (*f).readable as i32 == 0 as i32 {
        return -(1 as i32);
    }
    if (*f).type_0 as u32 == FD_PIPE as i32 as u32 {
        r = piperead((*f).pipe, addr, n)
    } else if (*f).type_0 as u32 == FD_DEVICE as i32 as u32 {
        if ((*f).major as i32) < 0 as i32
            || (*f).major as i32 >= NDEV
            || devsw[(*f).major as usize].read.is_none()
        {
            return -(1 as i32);
        }
        r = devsw[(*f).major as usize]
            .read
            .expect("non-null function pointer")(1 as i32, addr, n)
    } else if (*f).type_0 as u32 == FD_INODE as i32 as u32 {
        ilock((*f).ip);
        r = readi((*f).ip, 1 as i32, addr, (*f).off, n as u32);
        if r > 0 as i32 {
            (*f).off = ((*f).off as u32).wrapping_add(r as u32) as u32 as u32
        }
        iunlock((*f).ip);
    } else {
        panic(b"fileread\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    r
}
/// Write to file f.
/// addr is a user virtual address.
#[no_mangle]
pub unsafe extern "C" fn filewrite(mut f: *mut File, mut addr: u64, mut n: i32) -> i32 {
    let mut r: i32 = 0;
    let mut ret: i32 = 0;
    if (*f).writable as i32 == 0 as i32 {
        return -1;
    }
    if (*f).type_0 as u32 == FD_PIPE as i32 as u32 {
        ret = pipewrite((*f).pipe, addr, n)
    } else if (*f).type_0 as u32 == FD_DEVICE as i32 as u32 {
        if ((*f).major as i32) < 0 as i32
            || (*f).major as i32 >= NDEV
            || devsw[(*f).major as usize].write.is_none()
        {
            return -1;
        }
        ret = devsw[(*f).major as usize]
            .write
            .expect("non-null function pointer")(1 as i32, addr, n)
    } else if (*f).type_0 as u32 == FD_INODE as i32 as u32 {
        // write a few blocks at a time to avoid exceeding
        // the maximum log transaction size, including
        // i-node, indirect block, allocation blocks,
        // and 2 blocks of slop for non-aligned writes.
        // this really belongs lower down, since writei()
        // might be writing a device like the console.
        let mut max: i32 = (MAXOPBLOCKS - 1 - 1 - 2) / 2 * BSIZE;
        let mut i: i32 = 0;
        while i < n {
            let mut n1: i32 = n - i;
            if n1 > max {
                n1 = max
            }
            begin_op();
            ilock((*f).ip);
            r = writei(
                (*f).ip,
                1 as i32,
                addr.wrapping_add(i as u64),
                (*f).off,
                n1 as u32,
            );
            if r > 0 as i32 {
                (*f).off = ((*f).off as u32).wrapping_add(r as u32) as u32
            }
            iunlock((*f).ip);
            end_op();
            if r < 0 as i32 {
                break;
            }
            if r != n1 {
                panic(
                    b"short filewrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                );
            }
            i += r
        }
        ret = if i == n { n } else { -(1 as i32) }
    } else {
        panic(b"filewrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ret
}
