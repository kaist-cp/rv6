//! Support functions for system calls that involve file descriptors.

use core::{
    cell::UnsafeCell,
    cmp,
    mem::{self, ManuallyDrop},
    ops::Deref,
    ops::DerefMut,
};

use crate::{
    arch::addr::UVAddr,
    arena::{Arena, ArenaObject, ArenaRc, ArrayArena},
    fs::{FileSystem, InodeGuard, RcInode, Ufs},
    hal::hal,
    lock::SpinLock,
    param::{BSIZE, MAXOPBLOCKS, NFILE},
    pipe::AllocatedPipe,
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

pub enum FileType {
    None,
    Pipe {
        pipe: AllocatedPipe,
    },
    Inode {
        inner: InodeFileType,
    },
    Device {
        ip: RcInode<<Ufs as FileSystem>::InodeInner>,
        major: u16,
    },
}

/// It has an inode and an offset.
///
/// # Safety
///
/// The offset should be accessed only when the inode is locked.
pub struct InodeFileType {
    pub ip: RcInode<<Ufs as FileSystem>::InodeInner>,
    // It should be accessed only when `ip` is locked.
    pub off: UnsafeCell<u32>,
}

/// It can be acquired when the inode of `InodeFileType` is locked. `ip` is the guard of the locked
/// inode. `off` is a mutable reference to the offset. Accessing `off` is guaranteed to be safe
/// since the inode is locked.
struct InodeFileTypeGuard<'a, I> {
    ip: ManuallyDrop<InodeGuard<'a, I>>,
    off: &'a mut u32,
}

pub struct File {
    pub typ: FileType,
    readable: bool,
    writable: bool,
}

pub type FileTable = SpinLock<ArrayArena<File, NFILE>>;

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<fn(UVAddr, i32, &mut KernelCtx<'_, '_>) -> i32>,
    pub write: Option<fn(UVAddr, i32, &mut KernelCtx<'_, '_>) -> i32>,
}

/// A reference counted smart pointer to a `File`.
pub type RcFile = ArenaRc<FileTable>;

impl Default for FileType {
    fn default() -> Self {
        Self::None
    }
}

impl InodeFileType {
    fn lock(
        &self,
        ctx: &KernelCtx<'_, '_>,
    ) -> InodeFileTypeGuard<'_, <Ufs as FileSystem>::InodeInner> {
        let ip = self.ip.lock(ctx);
        // SAFETY: `ip` is locked and `off` can be exclusively accessed.
        let off = unsafe { &mut *self.off.get() };
        InodeFileTypeGuard {
            ip: ManuallyDrop::new(ip),
            off,
        }
    }
}

impl<I> InodeFileTypeGuard<'_, I> {
    fn free(mut self, ctx: &KernelCtx<'_, '_>) {
        let ip = unsafe { ManuallyDrop::take(&mut self.ip) };
        ip.free(ctx);
        core::mem::forget(self);
    }
}

impl<'a, I> Deref for InodeFileTypeGuard<'a, I> {
    type Target = InodeGuard<'a, I>;

    fn deref(&self) -> &Self::Target {
        &self.ip
    }
}

impl<'a, I> DerefMut for InodeFileTypeGuard<'a, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ip
    }
}

impl<I> Drop for InodeFileTypeGuard<'_, I> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("InodeFileTypeGuard must never drop.");
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

    /// Get metadata about file self.
    /// addr is a user virtual address, pointing to a struct stat.
    pub fn stat(&self, addr: UVAddr, ctx: &mut KernelCtx<'_, '_>) -> Result<(), ()> {
        match &self.typ {
            FileType::Inode {
                inner: InodeFileType { ip, .. },
            }
            | FileType::Device { ip, .. } => {
                let st = ip.stat(ctx);
                ctx.proc_mut().memory_mut().copy_out(addr, &st)
            }
            _ => Err(()),
        }
    }

    /// Read from file self.
    /// addr is a user virtual address.
    pub fn read(&self, addr: UVAddr, n: i32, ctx: &mut KernelCtx<'_, '_>) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.read(addr, n as usize, ctx),
            FileType::Inode { inner } => {
                let mut ip = inner.lock(ctx);
                let curr_off = *ip.off;
                let ret = ip.read_user(addr, curr_off, n as u32, ctx);
                if let Ok(v) = ret {
                    *ip.off += v as u32;
                }
                ip.free(ctx);
                ret
            }
            FileType::Device { major, .. } => {
                let major = ctx.kernel().devsw().get(*major as usize).ok_or(())?;
                let read = major.read.ok_or(())?;
                Ok(read(addr, n, ctx) as usize)
            }
            FileType::None => panic!("File::read"),
        }
    }

    /// Write to file self.
    /// addr is a user virtual address.
    pub fn write(&self, addr: UVAddr, n: i32, ctx: &mut KernelCtx<'_, '_>) -> Result<usize, ()> {
        if !self.writable {
            return Err(());
        }

        match &self.typ {
            FileType::Pipe { pipe } => pipe.write(addr, n as usize, ctx),
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
                    let tx = ctx.kernel().fs().as_pin().get_ref().begin_tx(ctx);
                    let mut ip = inner.lock(ctx);
                    let curr_off = *ip.off;
                    let r = ip.write_user(
                        addr + bytes_written,
                        curr_off,
                        bytes_to_write as u32,
                        ctx,
                        &tx,
                    );
                    if let Ok(r) = r {
                        *ip.off += r as u32;
                    }
                    tx.end(ctx);
                    ip.free(ctx);
                    let r = r?;
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
            FileType::Device { major, .. } => {
                let major = ctx.kernel().devsw().get(*major as usize).ok_or(())?;
                let write = major.write.ok_or(())?;
                Ok(write(addr, n, ctx) as usize)
            }
            FileType::None => panic!("File::read"),
        }
    }
}

impl const Default for File {
    fn default() -> Self {
        Self::new(FileType::None, false, false)
    }
}

impl ArenaObject for File {
    type Ctx<'a, 'id: 'a> = &'a KernelCtx<'id, 'a>;

    fn finalize<'a, 'id: 'a, A: Arena>(&mut self, ctx: Self::Ctx<'a, 'id>) {
        let typ = mem::replace(&mut self.typ, FileType::None);
        match typ {
            FileType::Pipe { pipe } => {
                if let Some(page) = pipe.close(self.writable, ctx) {
                    hal().kmem().free(page);
                }
            }
            FileType::Inode {
                inner: InodeFileType { ip, .. },
            }
            | FileType::Device { ip, .. } => {
                let tx = ctx.kernel().fs().as_pin().get_ref().begin_tx(ctx);
                ip.free((&tx, ctx));
                tx.end(ctx);
            }
            _ => (),
        }
    }
}

impl FileTable {
    pub const fn new_ftable() -> Self {
        SpinLock::new("FTABLE", ArrayArena::<File, NFILE>::new())
    }

    /// Allocate a file structure.
    pub fn alloc_file(
        self: StrongPin<'_, Self>,
        typ: FileType,
        readable: bool,
        writable: bool,
    ) -> Result<RcFile, ()> {
        self.alloc(|| File::new(typ, readable, writable)).ok_or(())
    }
}

impl RcFile {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    pub fn fdalloc(self, ctx: &mut KernelCtx<'_, '_>) -> Result<i32, ()> {
        let proc_data = ctx.proc_mut().deref_mut_data();
        for (fd, f) in proc_data.open_files.iter_mut().enumerate() {
            if f.is_none() {
                *f = Some(self);
                return Ok(fd as i32);
            }
        }
        self.free(ctx);
        Err(())
    }
}
