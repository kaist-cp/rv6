//! Support functions for system calls that involve file descriptors.

use core::{cell::UnsafeCell, cmp, mem, ops::Deref, ops::DerefMut};

use crate::{
    arch::addr::UVAddr,
    arena::{Arena, ArenaObject, ArrayArena, Rc},
    bio::Bcache,
    fs::{FileSystem, InodeGuard, RcInode},
    kernel::kernel_builder,
    lock::Spinlock,
    param::{BSIZE, MAXOPBLOCKS, NFILE},
    pipe::AllocatedPipe,
    proc::CurrentProc,
};

pub enum FileType {
    None,
    Pipe { pipe: AllocatedPipe },
    Inode { inner: InodeFileType },
    Device { ip: RcInode, major: &'static Devsw },
}

/// It has an inode and an offset.
///
/// # Safety
///
/// The offset should be accessed only when the inode is locked.
pub struct InodeFileType {
    pub ip: RcInode,
    // It should be accessed only when `ip` is locked.
    pub off: UnsafeCell<u32>,
}

/// It can be acquired when the inode of `InodeFileType` is locked. `ip` is the guard of the locked
/// inode. `off` is a mutable reference to the offset. Accessing `off` is guaranteed to be safe
/// since the inode is locked.
struct InodeFileTypeGuard<'a> {
    ip: InodeGuard<'a>,
    off: &'a mut u32,
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
    pub read: Option<fn(_: UVAddr, _: i32) -> i32>,
    pub write: Option<fn(_: UVAddr, _: i32) -> i32>,
}

/// A reference counted smart pointer to a `File`.
pub type RcFile = Rc<FileTable>;

impl Default for FileType {
    fn default() -> Self {
        Self::None
    }
}

impl InodeFileType {
    fn lock(&self, fs: &FileSystem, bcache: &Bcache) -> InodeFileTypeGuard<'_> {
        let ip = self.ip.lock(fs, bcache);
        // SAFETY: `ip` is locked and `off` can be exclusively accessed.
        let off = unsafe { &mut *self.off.get() };
        InodeFileTypeGuard { ip, off }
    }
}

impl<'a> Deref for InodeFileTypeGuard<'a> {
    type Target = InodeGuard<'a>;

    fn deref(&self) -> &Self::Target {
        &self.ip
    }
}

impl<'a> DerefMut for InodeFileTypeGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ip
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
    pub fn stat(&self, addr: UVAddr, proc: &mut CurrentProc<'_>) -> Result<(), ()> {
        match &self.typ {
            FileType::Inode {
                inner: InodeFileType { ip, .. },
            }
            | FileType::Device { ip, .. } => {
                let st = ip.stat();
                proc.memory_mut().copy_out(addr, &st)
            }
            _ => Err(()),
        }
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub fn read(
        &self,
        addr: UVAddr,
        n: i32,
        proc: &mut CurrentProc<'_>,
        fs: &FileSystem,
        bcache: &Bcache,
    ) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.read(addr, n as usize, proc),
            FileType::Inode { inner } => {
                let mut ip = inner.lock(fs, bcache);
                let curr_off = *ip.off;
                let ret = ip.read_user(addr, curr_off, n as u32, proc, fs, bcache);
                if let Ok(v) = ret {
                    *ip.off += v as u32;
                }
                ret
            }
            FileType::Device { major, .. } => major.read.ok_or(()).map(|f| f(addr, n) as usize),
            FileType::None => panic!("File::read"),
        }
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub fn write(
        &self,
        addr: UVAddr,
        n: i32,
        proc: &mut CurrentProc<'_>,
        fs: &FileSystem,
        bcache: &Bcache,
    ) -> Result<usize, ()> {
        if !self.writable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.write(addr, n as usize, proc),
            FileType::Inode { inner } => {
                let n = n as usize;

                // write a few blocks at a time to avoid exceeding
                // the maximum log transaction size, including
                // i-node, indirect block, allocation blocks,
                // and 2 blocks of slop for non-aligned writes.
                // this really belongs lower down, since write()
                // might be writing a device like the console.
                let max = (MAXOPBLOCKS - 1 - 1 - 2) / 2 * BSIZE;

                let mut bytes_written: usize = 0;
                while bytes_written < n {
                    let bytes_to_write = cmp::min(n - bytes_written, max);
                    let tx = fs.begin_transaction();
                    let mut ip = inner.lock(fs, bcache);
                    let curr_off = *ip.off;
                    let r = ip
                        .write_user(
                            addr + bytes_written,
                            curr_off,
                            bytes_to_write as u32,
                            proc,
                            &tx,
                            fs,
                            bcache,
                        )
                        .map(|v| {
                            *ip.off += v as u32;
                            v
                        })?;
                    if r != bytes_to_write {
                        // error from write_user
                        break;
                    }
                    bytes_written += r;
                }
                if bytes_written != n {
                    return Err(());
                }
                Ok(n)
            }
            FileType::Device { major, .. } => major.write.ok_or(()).map(|f| f(addr, n) as usize),
            FileType::None => panic!("File::read"),
        }
    }
}

impl const Default for File {
    fn default() -> Self {
        Self::zero()
    }
}

impl ArenaObject for File {
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>) {
        // SAFETY: `FileTable` does not use `Arena::find_or_alloc`.
        unsafe {
            A::reacquire_after(guard, || {
                let typ = mem::replace(&mut self.typ, FileType::None);
                match typ {
                    FileType::Pipe { pipe } => {
                        if let Some(page) = pipe.close(self.writable) {
                            // TODO: remove kernel_builder()
                            kernel_builder().kmem.free(page);
                        }
                    }
                    FileType::Inode {
                        inner: InodeFileType { ip, .. },
                    }
                    | FileType::Device { ip, .. } => {
                        // TODO(https://github.com/kaist-cp/rv6/issues/290)
                        // The inode ip will be dropped by drop(ip). Deallocation
                        // of an inode may cause disk write operations, so we must
                        // begin a transaction here.
                        // TODO: remove kernel_builder()
                        let _tx = kernel_builder().file_system.begin_transaction();
                        drop(ip);
                    }
                    _ => (),
                }
            });
        }
    }
}

impl FileTable {
    pub const fn zero() -> Self {
        Spinlock::new("FTABLE", ArrayArena::<File, NFILE>::new())
    }

    /// Allocate a file structure.
    pub fn alloc_file(&self, typ: FileType, readable: bool, writable: bool) -> Result<RcFile, ()> {
        // TODO(https://github.com/kaist-cp/rv6/issues/372): idiomatic initialization.
        self.alloc(|p| *p = File::new(typ, readable, writable))
            .ok_or(())
    }
}
