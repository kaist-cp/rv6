//! Support functions for system calls that involve file descriptors.
use crate::libc;
use crate::{
    fs::{stati, BSIZE},
    log::{begin_op, end_op},
    param::{MAXOPBLOCKS, NDEV, NFILE},
    pipe::Pipe,
    printf::panic,
    proc::{myproc, Proc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::Stat,
    vm::copyout,
};
use core::ptr;

pub const CONSOLE: isize = 1;

#[derive(Copy, Clone)]
pub struct File {
    pub typ: u32,

    /// reference count
    ref_0: i32,

    pub readable: libc::CChar,
    pub writable: libc::CChar,

    /// FD_PIPE
    pub pipe: *mut Pipe,

    /// FD_INODE and FD_DEVICE
    pub ip: *mut Inode,

    /// FD_INODE
    pub off: u32,

    /// FD_DEVICE
    pub major: i16,
}

/// in-memory copy of an inode
pub struct Inode {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    /// Reference count
    pub ref_0: i32,

    /// protects everything below here
    pub lock: Sleeplock,

    /// inode has been read from disk?
    pub valid: i32,

    /// copy of disk inode
    pub typ: i16,
    pub major: i16,
    pub minor: i16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; 13],
}

pub const FD_DEVICE: u32 = 3;
pub const FD_INODE: u32 = 2;
pub const FD_PIPE: u32 = 1;
pub const FD_NONE: u32 = 0;

struct Ftable {
    lock: Spinlock,
    file: [File; NFILE as usize],
}

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
    pub write: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
}

impl File {
    /// Allocate a file structure.
    pub unsafe fn alloc() -> *mut File {
        let mut f: *mut File = ptr::null_mut();
        FTABLE.lock.acquire();
        f = FTABLE.file.as_mut_ptr();
        while f < FTABLE.file.as_mut_ptr().offset(NFILE as isize) {
            if (*f).ref_0 == 0 {
                (*f).ref_0 = 1;
                FTABLE.lock.release();
                return f;
            }
            f = f.offset(1)
        }
        FTABLE.lock.release();
        ptr::null_mut()
    }

    /// Increment ref count for file self.
    pub unsafe fn dup(&mut self) -> *mut File {
        FTABLE.lock.acquire();
        if (*self).ref_0 < 1 {
            panic(b"File::dup\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        (*self).ref_0 += 1;
        FTABLE.lock.release();
        self
    }

    /// Close file self.  (Decrement ref count, close when reaches 0.)
    pub unsafe fn close(&mut self) {
        let mut ff: File = File::zeroed();
        FTABLE.lock.acquire();
        if (*self).ref_0 < 1 {
            panic(b"File::close\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        (*self).ref_0 -= 1;
        if (*self).ref_0 > 0 {
            FTABLE.lock.release();
            return;
        }
        ff = *self;
        (*self).ref_0 = 0;
        (*self).typ = FD_NONE;
        FTABLE.lock.release();
        if ff.typ as u32 == FD_PIPE as i32 as u32 {
            (*ff.pipe).close(ff.writable as i32);
        } else if ff.typ as u32 == FD_INODE as i32 as u32
            || ff.typ as u32 == FD_DEVICE as i32 as u32
        {
            begin_op();
            (*ff.ip).put();
            end_op();
        };
    }

    /// Get metadata about file self.
    /// addr is a user virtual address, pointing to a struct stat.
    pub unsafe fn stat(&mut self, addr: usize) -> i32 {
        let p: *mut Proc = myproc();
        let mut st: Stat = Default::default();
        if (*self).typ as u32 == FD_INODE as i32 as u32
            || (*self).typ as u32 == FD_DEVICE as i32 as u32
        {
            (*(*self).ip).lock();
            stati((*self).ip, &mut st);
            (*(*self).ip).unlock();
            if copyout(
                (*p).pagetable,
                addr,
                &mut st as *mut Stat as *mut libc::CChar,
                ::core::mem::size_of::<Stat>() as usize,
            ) < 0
            {
                return -1;
            }
            return 0;
        }
        -1
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub unsafe fn read(&mut self, addr: usize, n: i32) -> i32 {
        let mut r: i32 = 0;
        if (*self).readable as i32 == 0 {
            return -1;
        }
        if (*self).typ as u32 == FD_PIPE as i32 as u32 {
            r = (*(*self).pipe).read(addr, n)
        } else if (*self).typ as u32 == FD_DEVICE as i32 as u32 {
            if ((*self).major as i32) < 0
                || (*self).major as i32 >= NDEV
                || DEVSW[(*self).major as usize].read.is_none()
            {
                return -1;
            }
            r = DEVSW[(*self).major as usize]
                .read
                .expect("non-null function pointer")(1, addr, n)
        } else if (*self).typ as u32 == FD_INODE as i32 as u32 {
            (*(*self).ip).lock();
            r = (*(*self).ip).read(1, addr, (*self).off, n as u32);
            if r > 0 {
                (*self).off = ((*self).off as u32).wrapping_add(r as u32) as u32 as u32
            }
            (*(*self).ip).unlock();
        } else {
            panic(b"File::read\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        r
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> i32 {
        let mut r: i32 = 0;
        let mut ret: i32 = 0;
        if (*self).writable as i32 == 0 {
            return -1;
        }
        if (*self).typ as u32 == FD_PIPE as i32 as u32 {
            ret = (*(*self).pipe).write(addr, n)
        } else if (*self).typ as u32 == FD_DEVICE as i32 as u32 {
            if ((*self).major as i32) < 0
                || (*self).major as i32 >= NDEV
                || DEVSW[(*self).major as usize].write.is_none()
            {
                return -1;
            }
            ret = DEVSW[(*self).major as usize]
                .write
                .expect("non-null function pointer")(1, addr, n)
        } else if (*self).typ as u32 == FD_INODE as i32 as u32 {
            // write a few blocks at a time to avoid exceeding
            // the maximum log transaction size, including
            // i-node, indirect block, allocation blocks,
            // and 2 blocks of slop for non-aligned writes.
            // this really belongs lower down, since write()
            // might be writing a device like the console.
            let max = (MAXOPBLOCKS - 1 - 1 - 2) / 2 * BSIZE;
            let mut i: i32 = 0;
            while i < n {
                let mut n1: i32 = n - i;
                if n1 > max {
                    n1 = max
                }
                begin_op();
                (*(*self).ip).lock();
                r = (*(*self).ip).write(1, addr.wrapping_add(i as usize), (*self).off, n1 as u32);
                if r > 0 {
                    (*self).off = ((*self).off as u32).wrapping_add(r as u32) as u32
                }
                (*(*self).ip).unlock();
                end_op();
                if r < 0 {
                    break;
                }
                if r != n1 {
                    panic(
                        b"short File::write\x00" as *const u8 as *const libc::CChar
                            as *mut libc::CChar,
                    );
                }
                i += r
            }
            ret = if i == n { n } else { -1 }
        } else {
            panic(b"File::write\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        ret
    }

    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            typ: FD_NONE,
            ref_0: 0,
            readable: 0,
            writable: 0,
            pipe: ptr::null_mut(),
            ip: ptr::null_mut(),
            off: 0,
            major: 0,
        }
    }
}

impl Ftable {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
            file: [File::zeroed(); NFILE as usize],
        }
    }
}

/// Support functions for system calls that involve file descriptors.
pub static mut DEVSW: [Devsw; NDEV as usize] = [Devsw {
    read: None,
    write: None,
}; NDEV as usize];

static mut FTABLE: Ftable = Ftable::zeroed();

pub unsafe fn fileinit() {
    FTABLE
        .lock
        .initlock(b"FTABLE\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
}
