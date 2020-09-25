//! Support functions for system calls that involve file descriptors.
use crate::{
    fs::{stati, BSIZE},
    log::{begin_op, end_op},
    param::{MAXOPBLOCKS, NDEV, NFILE},
    pipe::AllocatedPipe,
    pool::{PoolRef, RcPool, TaggedBox},
    proc::{myproc, Proc},
    sleeplock::Sleeplock,
    spinlock::Spinlock,
    stat::Stat,
};
use core::cmp;
use core::convert::TryFrom;

pub struct File {
    pub typ: FileType,
    readable: bool,
    writable: bool,
}

// TODO: will be infered as we wrap *mut Pipe and *mut Inode.
unsafe impl Send for File {}

pub struct InodeInner {
    /// protects everything below here
    pub lock: Sleeplock,

    /// inode has been read from disk?
    pub valid: i32,

    /// copy of disk inode
    pub typ: i16,
    pub major: u16,
    pub minor: u16,
    pub nlink: i16,
    pub size: u32,
    pub addrs: [u32; 13],
}

/// in-memory copy of an inode
pub struct Inode {
    /// Device number
    pub dev: u32,

    /// Inode number
    pub inum: u32,

    /// Reference count
    pub ref_0: i32,

    pub inner: InodeInner,
}

pub enum FileType {
    None,
    Pipe { pipe: AllocatedPipe },
    Inode { ip: *mut Inode, off: u32 },
    Device { ip: *mut Inode, major: u16 },
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

static FTABLE: Spinlock<RcPool<File, NFILE>> = Spinlock::new("FTABLE", RcPool::new());

pub struct FTableRef(());

// SAFETY: We have only one `PoolRef` pointing `FTABLE`.
unsafe impl PoolRef for FTableRef {
    type Target = Spinlock<RcPool<File, NFILE>>;
    fn deref() -> &'static Self::Target {
        &FTABLE
    }
}

pub type RcFile = TaggedBox<FTableRef, File>;

impl RcFile {
    /// Allocate a file structure.
    pub fn alloc(readable: bool, writable: bool) -> Option<Self> {
        // TODO: idiomatic initialization.
        FTableRef::alloc(File::init(readable, writable))
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
    pub unsafe fn stat(&mut self, addr: usize) -> Result<(), ()> {
        let p: *mut Proc = myproc();
        let mut st: Stat = Default::default();

        match self.typ {
            FileType::Inode { ip, .. } | FileType::Device { ip, .. } => {
                (*ip).lock();
                stati(ip, &mut st);
                (*ip).unlock();
                if (*p)
                    .pagetable
                    .assume_init_mut()
                    .copyout(
                        addr,
                        &mut st as *mut Stat as *mut u8,
                        ::core::mem::size_of::<Stat>() as usize,
                    )
                    .is_err()
                {
                    Err(())
                } else {
                    Ok(())
                }
            }
            _ => Err(()),
        }
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub unsafe fn read(&mut self, addr: usize, n: i32) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        // Use &mut self.typ because read() "changes" FileType::Inode.off during holding ip's lock.
        match &mut self.typ {
            FileType::Pipe { pipe } => pipe.read(addr, usize::try_from(n).unwrap_or(0)),
            FileType::Inode { ip, off } => {
                (**ip).lock();
                let r = (**ip).read(1, addr, *off, n as u32);
                if r > 0 {
                    *off = off.wrapping_add(r as u32);
                }
                (**ip).unlock();
                Ok(r as usize)
            }
            FileType::Device { major, .. } => DEVSW
                .get(*major as usize)
                .and_then(|dev| Some(dev.read?(1, addr, n) as usize))
                .ok_or(()),
            _ => panic!("File::read"),
        }
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> Result<usize, ()> {
        if !self.writable {
            return Err(());
        }

        // Use &mut self.typ because write() "changes" FileType::Inode.off during holding ip's lock.
        match &mut self.typ {
            FileType::Pipe { pipe } => pipe.write(addr, usize::try_from(n).unwrap_or(0)),
            FileType::Inode { ip, off } => {
                // write a few blocks at a time to avoid exceeding
                // the maximum log transaction size, including
                // i-node, indirect block, allocation blocks,
                // and 2 blocks of slop for non-aligned writes.
                // this really belongs lower down, since write()
                // might be writing a device like the console.
                let max = (MAXOPBLOCKS - 1 - 1 - 2) / 2 * BSIZE;
                for bytes_written in (0..n).step_by(max) {
                    let bytes_to_write = cmp::min(n - bytes_written, max as i32);
                    begin_op();
                    (**ip).lock();

                    let r = (**ip).write(
                        1,
                        addr.wrapping_add(bytes_written as usize),
                        *off,
                        bytes_to_write as u32,
                    );
                    if r > 0 {
                        *off = off.wrapping_add(r as u32);
                    }
                    (**ip).unlock();
                    end_op();
                    if r < 0 {
                        return Err(());
                    }
                    if r != bytes_to_write {
                        panic!("short File::write");
                    }
                }
                Ok(n as usize)
            }
            FileType::Device { major, .. } => DEVSW
                .get(*major as usize)
                .and_then(|dev| Some(dev.write?(1, addr, n) as usize))
                .ok_or(()),
            _ => panic!("File::read"),
        }
    }

    // TODO: transient measure
    pub const fn init(readable: bool, writable: bool) -> Self {
        Self {
            typ: FileType::None,
            readable,
            writable,
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // TODO: Reasoning why.
        unsafe {
            match self.typ {
                FileType::Pipe { mut pipe } => pipe.close(self.writable),
                FileType::Inode { ip, .. } | FileType::Device { ip, .. } => {
                    begin_op();
                    (*ip).put();
                    end_op();
                }
                _ => (),
            }
        }
    }
}
