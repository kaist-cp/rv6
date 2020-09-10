//! Support functions for system calls that involve file descriptors.
use crate::{
    fs::{stati, BSIZE},
    log::{begin_op, end_op},
    param::{MAXOPBLOCKS, NDEV},
    pipe::AllocatedPipe,
    pool::{PoolRef, RcPool, TaggedBox},
    proc::{myproc, Proc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::Stat,
    vm::copyout,
};
use core::{cmp, ptr};

pub const CONSOLE: usize = 1;

pub struct File {
    pub typ: Filetype,

    /// reference count
    ref_0: i32,

    pub readable: bool,
    pub writable: bool,

    /// Filetype::PIPE
    pub pipe: AllocatedPipe,

    /// Filetype::INODE and Filetype::DEVICE
    pub ip: *mut Inode,

    /// Filetype::INODE
    pub off: u32,

    /// Filetype::DEVICE
    pub major: i16,
}

// TODO: will be infered as we wrap *mut Pipe and *mut Inode.
unsafe impl Send for File {}

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

#[derive(Copy, Clone, PartialEq)]
pub enum Filetype {
    NONE,
    PIPE,
    INODE,
    DEVICE,
}

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

static FTABLE: Spinlock<RcPool<File>> = Spinlock::new("FTABLE", RcPool::new());

pub struct FTableRef(());

// SAFETY: We have only one `PoolRef` pointing `FTABLE`.
unsafe impl PoolRef for FTableRef {
    type Target = Spinlock<RcPool<File>>;
    fn deref() -> &'static Self::Target {
        &FTABLE
    }
}

pub type RcFile = TaggedBox<FTableRef, File>;

impl RcFile {
    /// Allocate a file structure.
    pub fn alloc() -> Option<Self> {
        // TODO: idiomatic initialization.
        FTableRef::alloc(File::zeroed())
    }

    /// Increment reference count of the file.
    pub fn dup(&self) -> Self {
        // SAFETY: `self` is allocated from `FTABLE`, ensured by given type parameter `FTableRef`.
        unsafe { RcFile::from_unchecked(FTABLE.lock().dup(&*self)) }
    }
}

impl File {
    /// Get metadata about file self.
    /// addr is a user virtual address, pointing to a struct stat.
    pub unsafe fn stat(&mut self, addr: usize) -> i32 {
        let p: *mut Proc = myproc();
        let mut st: Stat = Default::default();
        if self.typ == Filetype::INODE || self.typ == Filetype::DEVICE {
            (*self.ip).lock();
            stati(self.ip, &mut st);
            (*self.ip).unlock();
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
        if !self.readable {
            return -1;
        }

        if self.typ == Filetype::PIPE {
            self.pipe.read(addr, n)
        } else if self.typ == Filetype::DEVICE {
            if (self.major) < 0
                || self.major as usize >= NDEV
                || DEVSW[self.major as usize].read.is_none()
            {
                return -1;
            }
            DEVSW[self.major as usize]
                .read
                .expect("non-null function pointer")(1, addr, n)
        } else if self.typ == Filetype::INODE {
            (*self.ip).lock();
            let r = (*self.ip).read(1, addr, self.off, n as u32);
            if r > 0 {
                self.off = (self.off).wrapping_add(r as u32)
            }
            (*self.ip).unlock();
            r
        } else {
            panic!("File::read");
        }
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> i32 {
        if !self.writable {
            return -1;
        }
        if self.typ == Filetype::PIPE {
            self.pipe.write(addr, n)
        } else if self.typ == Filetype::DEVICE {
            if (self.major) < 0
                || self.major as usize >= NDEV
                || DEVSW[self.major as usize].write.is_none()
            {
                return -1;
            }
            DEVSW[self.major as usize]
                .write
                .expect("non-null function pointer")(1, addr, n)
        } else if self.typ == Filetype::INODE {
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
                (*self.ip).lock();
                let r = (*self.ip).write(1, addr.wrapping_add(i as usize), self.off, n1 as u32);
                if r > 0 {
                    self.off = (self.off).wrapping_add(r as u32)
                }
                (*self.ip).unlock();
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
            typ: Filetype::NONE,
            ref_0: 0,
            readable: false,
            writable: false,
            pipe: AllocatedPipe::zeroed(),
            ip: ptr::null_mut(),
            off: 0,
            major: 0,
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // TODO: Reasoning why.
        unsafe {
            match self.typ {
                Filetype::PIPE => self.pipe.close(self.writable),
                Filetype::INODE | Filetype::DEVICE => {
                    begin_op();
                    (*self.ip).put();
                    end_op();
                }
                _ => (),
            }
        }
    }
}
