//! Support functions for system calls that involve file descriptors.
use crate::{
    fs::{stati, BSIZE},
    log::{begin_op, end_op},
    param::{MAXOPBLOCKS, NDEV, NFILE},
    pipe::Pipe,
    proc::{myproc, Proc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::Stat,
    vm::copyout,
};
use core::{ops::DerefMut, ptr, cmp};

pub const CONSOLE: usize = 1;

#[derive(Copy, Clone)]
pub struct File {
    pub typ: u32,

    /// reference count
    ref_0: i32,

    pub readable: bool,
    pub writable: bool,

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

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
    pub write: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
}

pub static mut DEVSW: [Devsw; NDEV] = [Devsw {
    read: None,
    write: None,
}; NDEV];

static mut FTABLE: Spinlock<[File; NFILE]> = Spinlock::new("FTABLE", [File::zeroed(); NFILE]);

impl File {
    /// Allocate a file structure.
    pub unsafe fn alloc() -> *mut File {
        let mut file = FTABLE.lock();
        for f in &mut file.deref_mut()[..] {
            if (*f).ref_0 == 0 {
                (*f).ref_0 = 1;
                return f;
            }
        }
        ptr::null_mut()
    }

    /// Increment ref count for file self.
    pub unsafe fn dup(&mut self) -> *mut File {
        let _file = FTABLE.lock();
        if (*self).ref_0 < 1 {
            panic!("File::dup");
        }
        (*self).ref_0 += 1;
        self
    }

    /// Close file self.  (Decrement ref count, close when reaches 0.)
    pub unsafe fn close(&mut self) {
        let file = FTABLE.lock();
        if (*self).ref_0 < 1 {
            panic!("File::close");
        }
        (*self).ref_0 -= 1;
        if (*self).ref_0 > 0 {
            return;
        }
        let ff: File = *self;
        (*self).ref_0 = 0;
        (*self).typ = FD_NONE;
        drop(file);
        if ff.typ == FD_PIPE {
            (*ff.pipe).close(ff.writable);
        } else if ff.typ == FD_INODE || ff.typ == FD_DEVICE {
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
        if (*self).typ == FD_INODE || (*self).typ == FD_DEVICE {
            (*(*self).ip).lock();
            stati((*self).ip, &mut st);
            (*(*self).ip).unlock();
            if copyout(
                (*p).pagetable,
                addr,
                &mut st as *mut Stat as *mut u8,
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
        if !(*self).readable {
            return -1;
        }

        if (*self).typ == FD_PIPE {
            (*(*self).pipe).read(addr, n)
        } else if (*self).typ == FD_DEVICE {
            if ((*self).major) < 0
                || (*self).major as usize >= NDEV
                || DEVSW[(*self).major as usize].read.is_none()
            {
                return -1;
            }
            DEVSW[(*self).major as usize]
                .read
                .expect("non-null function pointer")(1, addr, n)
        } else if (*self).typ == FD_INODE {
            (*(*self).ip).lock();
            let r = (*(*self).ip).read(1, addr, (*self).off, n as u32);
            if r > 0 {
                (*self).off = ((*self).off).wrapping_add(r as u32)
            }
            (*(*self).ip).unlock();
            r
        } else {
            panic!("File::read");
        }
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> i32 {
        if !(*self).writable {
            return -1;
        }
        if (*self).typ == FD_PIPE {
            (*(*self).pipe).write(addr, n)
        } else if (*self).typ == FD_DEVICE {
            if ((*self).major) < 0
                || (*self).major as usize >= NDEV
                || DEVSW[(*self).major as usize].write.is_none()
            {
                return -1;
            }
            DEVSW[(*self).major as usize]
                .write
                .expect("non-null function pointer")(1, addr, n)
        } else if (*self).typ == FD_INODE {
            // write a few blocks at a time to avoid exceeding
            // the maximum log transaction size, including
            // i-node, indirect block, allocation blocks,
            // and 2 blocks of slop for non-aligned writes.
            // this really belongs lower down, since write()
            // might be writing a device like the console.
            let max = (MAXOPBLOCKS - 1 - 1 - 2) / 2 * BSIZE;
            let mut i: i32 = 0;
            while i < n {
                // TODO : rename `n1`
                let n1 = cmp::min(n - i, max as i32);
                begin_op();
                (*(*self).ip).lock();
                let r =
                    (*(*self).ip).write(1, addr.wrapping_add(i as usize), (*self).off, n1 as u32);
                if r > 0 {
                    (*self).off = ((*self).off).wrapping_add(r as u32)
                }
                (*(*self).ip).unlock();
                end_op();
                if r < 0 {
                    break;
                }
                if r != n1 {
                    panic!("short File::write");
                }
                i += r
            }
            if i == n {
                n
            } else {
                -1
            }
        } else {
            panic!("File::write");
        }
    }

    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            typ: FD_NONE,
            ref_0: 0,
            readable: false,
            writable: false,
            pipe: ptr::null_mut(),
            ip: ptr::null_mut(),
            off: 0,
            major: 0,
        }
    }
}
