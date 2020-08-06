use crate::libc;
use crate::{
    fs::{stati, BSIZE},
    log::{begin_op, end_op},
    param::{MAXOPBLOCKS, NDEV, NFILE},
    pipe::Pipe,
    printf::panic,
    proc::{myproc, proc_0},
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
    ref_0: i32,
    pub readable: libc::c_char,
    pub writable: libc::c_char,
    pub pipe: *mut Pipe,
    pub ip: *mut Inode,
    pub off: u32,
    pub major: i16,
}

/// FD_DEVICE
/// in-memory copy of an inode
#[derive(Copy, Clone)]
pub struct Inode {
    pub dev: u32,
    pub inum: u32,
    pub ref_0: i32,
    pub lock: Sleeplock,
    pub valid: i32,
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

#[derive(Copy, Clone)]
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
    /// Increment ref count for file self.
    pub unsafe fn dup(&mut self) -> *mut File {
        ftable.lock.acquire();
        if (*self).ref_0 < 1 as i32 {
            panic(b"File::dup\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        (*self).ref_0 += 1;
        ftable.lock.release();
        self
    }

    /// Close file self.  (Decrement ref count, close when reaches 0.)
    pub unsafe fn close(&mut self) {
        let mut ff: File = File::zeroed();
        ftable.lock.acquire();
        if (*self).ref_0 < 1 as i32 {
            panic(b"File::close\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        (*self).ref_0 -= 1;
        if (*self).ref_0 > 0 as i32 {
            ftable.lock.release();
            return;
        }
        ff = *self;
        (*self).ref_0 = 0 as i32;
        (*self).typ = FD_NONE;
        ftable.lock.release();
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
    pub unsafe fn stat(&mut self, mut addr: usize) -> i32 {
        let mut p: *mut proc_0 = myproc();
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
                &mut st as *mut Stat as *mut libc::c_char,
                ::core::mem::size_of::<Stat>() as usize,
            ) < 0 as i32
            {
                return -(1 as i32);
            }
            return 0 as i32;
        }
        -(1 as i32)
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub unsafe fn read(&mut self, mut addr: usize, mut n: i32) -> i32 {
        let mut r: i32 = 0;
        if (*self).readable as i32 == 0 as i32 {
            return -(1 as i32);
        }
        if (*self).typ as u32 == FD_PIPE as i32 as u32 {
            r = (*(*self).pipe).read(addr, n)
        } else if (*self).typ as u32 == FD_DEVICE as i32 as u32 {
            if ((*self).major as i32) < 0 as i32
                || (*self).major as i32 >= NDEV
                || devsw[(*self).major as usize].read.is_none()
            {
                return -(1 as i32);
            }
            r = devsw[(*self).major as usize]
                .read
                .expect("non-null function pointer")(1 as i32, addr, n)
        } else if (*self).typ as u32 == FD_INODE as i32 as u32 {
            (*(*self).ip).lock();
            r = (*(*self).ip).read(1 as i32, addr, (*self).off, n as u32);
            if r > 0 as i32 {
                (*self).off = ((*self).off as u32).wrapping_add(r as u32) as u32 as u32
            }
            (*(*self).ip).unlock();
        } else {
            panic(b"File::read\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        r
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&mut self, mut addr: usize, mut n: i32) -> i32 {
        let mut r: i32 = 0;
        let mut ret: i32 = 0;
        if (*self).writable as i32 == 0 as i32 {
            return -1;
        }
        if (*self).typ as u32 == FD_PIPE as i32 as u32 {
            ret = (*(*self).pipe).write(addr, n)
        } else if (*self).typ as u32 == FD_DEVICE as i32 as u32 {
            if ((*self).major as i32) < 0 as i32
                || (*self).major as i32 >= NDEV
                || devsw[(*self).major as usize].write.is_none()
            {
                return -1;
            }
            ret = devsw[(*self).major as usize]
                .write
                .expect("non-null function pointer")(1 as i32, addr, n)
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
                r = (*(*self).ip).write(
                    1 as i32,
                    addr.wrapping_add(i as usize),
                    (*self).off,
                    n1 as u32,
                );
                if r > 0 as i32 {
                    (*self).off = ((*self).off as u32).wrapping_add(r as u32) as u32
                }
                (*(*self).ip).unlock();
                end_op();
                if r < 0 as i32 {
                    break;
                }
                if r != n1 {
                    panic(
                        b"short File::write\x00" as *const u8 as *const libc::c_char
                            as *mut libc::c_char,
                    );
                }
                i += r
            }
            ret = if i == n { n } else { -(1 as i32) }
        } else {
            panic(b"File::write\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        ret
    }

    /// Allocate a file structure.
    pub unsafe fn alloc() -> *mut File {
        let mut f: *mut File = ptr::null_mut();
        ftable.lock.acquire();
        f = ftable.file.as_mut_ptr();
        while f < ftable.file.as_mut_ptr().offset(NFILE as isize) {
            if (*f).ref_0 == 0 as i32 {
                (*f).ref_0 = 1 as i32;
                ftable.lock.release();
                return f;
            }
            f = f.offset(1)
        }
        ftable.lock.release();
        ptr::null_mut()
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
pub static mut devsw: [Devsw; NDEV as usize] = [Devsw {
    read: None,
    write: None,
}; NDEV as usize];

static mut ftable: Ftable = Ftable::zeroed();

pub unsafe fn fileinit() {
    ftable
        .lock
        .initlock(b"ftable\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
