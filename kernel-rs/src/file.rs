//! Support functions for system calls that involve file descriptors.
use crate::{
    fs::{fs, Inode, BSIZE},
    kernel::kernel,
    param::{MAXOPBLOCKS, NFILE},
    pipe::AllocatedPipe,
    pool::{PoolRef, RcPool, TaggedBox},
    proc::{myproc, Proc},
    spinlock::Spinlock,
    stat::Stat,
};
use core::{cell::Cell, cmp, convert::TryFrom};
pub struct File {
    pub typ: FileType,
    readable: bool,
    writable: bool,
}

// TODO: will be infered as we wrap *mut Pipe and *mut Inode.
unsafe impl Send for File {}

pub enum FileType {
    None,
    Pipe { pipe: AllocatedPipe },
    Inode { ip: *mut Inode, off: Cell<u32> },
    Device { ip: *mut Inode, major: u16 },
}

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
    pub write: Option<unsafe fn(_: i32, _: usize, _: i32) -> i32>,
}

pub struct FTableRef(());

// SAFETY: We have only one `PoolRef` pointing `FTABLE`.
unsafe impl PoolRef for FTableRef {
    type Target = Spinlock<RcPool<File, NFILE>>;
    fn deref() -> &'static Self::Target {
        &kernel().ftable
    }
}

pub type RcFile = TaggedBox<FTableRef, File>;

impl RcFile {
    /// Allocate a file structure.
    pub fn alloc(readable: bool, writable: bool) -> Option<Self> {
        // TODO: idiomatic initialization.
        FTableRef::alloc(File::new(readable, writable))
    }

    /// Increment reference count of the file.
    pub fn dup(&self) -> Self {
        // SAFETY: `self` is allocated from `FTABLE`, ensured by given type parameter `FTableRef`.
        unsafe { RcFile::from_unchecked(kernel().ftable.lock().dup(&*self)) }
    }
}

impl File {
    pub const fn new(readable: bool, writable: bool) -> Self {
        Self {
            typ: FileType::None,
            readable,
            writable,
        }
    }

    /// Get metadata about file self.
    /// addr is a user virtual address, pointing to a struct stat.
    pub unsafe fn stat(&self, addr: usize) -> Result<(), ()> {
        let p: *mut Proc = myproc();

        match self.typ {
            FileType::Inode { ip, .. } | FileType::Device { ip, .. } => {
                let mut st = (*ip).lock().stat();
                (*p).pagetable.assume_init_mut().copyout(
                    addr,
                    &mut st as *mut Stat as *mut u8,
                    ::core::mem::size_of::<Stat>() as usize,
                )
            }
            _ => Err(()),
        }
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub unsafe fn read(&self, addr: usize, n: i32) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.read(addr, usize::try_from(n).unwrap_or(0)),
            FileType::Inode { ip, off } => {
                let mut ip = (**ip).lock();
                let curr_off = off.get();
                let ret = ip.read(true, addr, curr_off, n as u32);
                if let Ok(v) = ret {
                    off.set(curr_off.wrapping_add(v as u32));
                }
                drop(ip);
                ret
            }
            FileType::Device { major, .. } => kernel()
                .devsw
                .get(*major as usize)
                .and_then(|dev| Some(dev.read?(1, addr, n) as usize))
                .ok_or(()),
            _ => panic!("File::read"),
        }
    }
    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&self, addr: usize, n: i32) -> Result<usize, ()> {
        if !self.writable {
            return Err(());
        }

        match &self.typ {
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
                    fs().begin_op();
                    let mut ip = (**ip).lock();
                    let curr_off = off.get();
                    let bytes_written = ip
                        .write(
                            true,
                            addr.wrapping_add(bytes_written as usize),
                            curr_off,
                            bytes_to_write as u32,
                        )
                        .map(|v| {
                            off.set(curr_off.wrapping_add(v as u32));
                            v
                        });
                    drop(ip);
                    fs().end_op();
                    assert!(
                        bytes_written? == bytes_to_write as usize,
                        "short File::write"
                    );
                }
                Ok(n as usize)
            }
            FileType::Device { major, .. } => kernel()
                .devsw
                .get(*major as usize)
                .and_then(|dev| Some(dev.write?(1, addr, n) as usize))
                .ok_or(()),
            _ => panic!("File::read"),
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
                    fs().begin_op();
                    (*ip).put();
                    fs().end_op();
                }
                _ => (),
            }
        }
    }
}
