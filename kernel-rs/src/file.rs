//! Support functions for system calls that involve file descriptors.

use crate::{
    arena::{Arena, ArenaObject, ArrayArena, ArrayEntry, Rc},
    fs::RcInode,
    kernel::kernel,
    param::{BSIZE, MAXOPBLOCKS, NFILE},
    pipe::AllocatedPipe,
    proc::{myproc, Proc},
    spinlock::Spinlock,
    stat::Stat,
    vm::UVAddr,
};
use core::{cell::UnsafeCell, cmp, convert::TryFrom, mem, ops::Deref, slice};

pub enum FileType {
    None,
    Pipe {
        pipe: AllocatedPipe,
    },
    Inode {
        ip: RcInode<'static>,
        off: UnsafeCell<u32>,
    },
    Device {
        ip: RcInode<'static>,
        major: u16,
    },
}

pub struct File {
    pub typ: FileType,
    readable: bool,
    writable: bool,
}

pub type FileTable = Spinlock<ArrayArena<File, NFILE>>;

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<unsafe fn(_: UVAddr, _: i32) -> i32>,
    pub write: Option<unsafe fn(_: UVAddr, _: i32) -> i32>,
}

pub type RcFile<'s> = Rc<FileTable, &'s FileTable>;

// TODO: will be infered as we wrap *mut Pipe and *mut Inode.
unsafe impl Send for File {}

impl Default for FileType {
    fn default() -> Self {
        Self::None
    }
}

impl File {
    pub const fn new(typ: FileType, readable: bool, writable: bool) -> Self {
        Self {
            typ,
            readable,
            writable,
        }
    }

    pub const fn zero() -> Self {
        Self::new(FileType::None, false, false)
    }

    /// Get metadata about file self.
    /// addr is a user virtual address, pointing to a struct stat.
    pub unsafe fn stat(&self, addr: UVAddr) -> Result<(), ()> {
        let p: *mut Proc = myproc();

        match &self.typ {
            FileType::Inode { ip, .. } | FileType::Device { ip, .. } => {
                let mut st = ip.stat();
                (*(*p).data.get()).pagetable.copy_out(
                    addr,
                    slice::from_raw_parts_mut(
                        &mut st as *mut Stat as *mut u8,
                        mem::size_of::<Stat>() as usize,
                    ),
                )
            }
            _ => Err(()),
        }
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub unsafe fn read(&self, addr: UVAddr, n: i32) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.read(addr, usize::try_from(n).unwrap_or(0)),
            FileType::Inode { ip, off } => {
                let mut ip = ip.deref().lock();
                let curr_off = *off.get();
                let ret = ip.read(addr, curr_off, n as u32);
                if let Ok(v) = ret {
                    *off.get() = curr_off.wrapping_add(v as u32);
                }
                drop(ip);
                ret
            }
            FileType::Device { major, .. } => kernel()
                .devsw
                .get(*major as usize)
                .and_then(|dev| Some(dev.read?(addr, n) as usize))
                .ok_or(()),
            FileType::None => panic!("File::read"),
        }
    }
    /// Write to file self.
    /// addr is a user virtual address.
    pub unsafe fn write(&self, addr: UVAddr, n: i32) -> Result<usize, ()> {
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

                // TODO(@kimjungwow) : To pass copyin() usertest, I reflect the commit on Nov 5, 2020 (below link).
                // https://github.com/mit-pdos/xv6-riscv/commit/5e392531c07966fd8a6bee50e3e357c553fb2a2f
                // This comment will be removed as we fetch upstream(mit-pdos)
                let mut bytes_written: usize = 0;
                while bytes_written < n as usize {
                    let bytes_to_write = cmp::min(n as usize - bytes_written, max);
                    let tx = kernel().file_system.begin_transaction();
                    let mut ip = ip.deref().lock();
                    let curr_off = *off.get();
                    let r = ip
                        .write(
                            addr + bytes_written as usize,
                            curr_off,
                            bytes_to_write as u32,
                            &tx,
                        )
                        .map(|v| {
                            *off.get() = curr_off.wrapping_add(v as u32);
                            v
                        })?;
                    if r != bytes_to_write as usize {
                        // error from InodeGuard::write
                        break;
                    }
                    bytes_written += r;
                }
                if bytes_written != n as usize {
                    return Err(());
                }
                Ok(n as usize)
            }
            FileType::Device { major, .. } => kernel()
                .devsw
                .get(*major as usize)
                .and_then(|dev| Some(dev.write?(addr, n) as usize))
                .ok_or(()),
            FileType::None => panic!("File::read"),
        }
    }
}

impl ArenaObject for File {
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>) {
        A::reacquire_after(guard, || {
            let typ = mem::replace(&mut self.typ, FileType::None);
            match typ {
                FileType::Pipe { mut pipe } => unsafe { pipe.close(self.writable) },
                FileType::Inode { ip, .. } | FileType::Device { ip, .. } => {
                    // TODO(rv6)
                    // The inode ip will be dropped by drop(ip). Deallocation
                    // of an inode may cause disk write operations, so we must
                    // begin a transaction here.
                    // https://github.com/kaist-cp/rv6/issues/290
                    let _tx = kernel().file_system.begin_transaction();
                    drop(ip);
                }
                _ => (),
            }
        });
    }
}

impl FileTable {
    pub const fn zero() -> Self {
        Spinlock::new(
            "FTABLE",
            ArrayArena::new(array![_ => ArrayEntry::new(File::zero()); NFILE]),
        )
    }

    /// Allocate a file structure.
    pub fn alloc_file(&self, typ: FileType, readable: bool, writable: bool) -> Result<RcFile<'_>, ()> {
        // TODO: idiomatic initialization.
        let inner = self.alloc(|p| {
            *p = File::new(typ, readable, writable);
        }).ok_or(())?;

        Ok(unsafe { Rc::from_unchecked(self, inner) })
    }
}
