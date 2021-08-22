//! Support functions for system calls that involve file descriptors.

use core::{
    cell::UnsafeCell,
    cmp,
    mem::{self, ManuallyDrop},
    ops::Deref,
    ops::DerefMut,
};

use crate::{
    addr::UVAddr,
    arena::{Arena, ArenaObject, ArenaRc, ArrayArena},
    fs::{DefaultFs, FileSystem, FileSystemExt, InodeGuard, RcInode},
    hal::hal,
    param::{BSIZE, MAXOPBLOCKS, NFILE},
    pipe::AllocatedPipe,
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

pub enum FileType {
    None,
    Pipe { pipe: AllocatedPipe },
    Inode { inner: InodeFileType },
    Device { ip: RcInode<DefaultFs>, major: u16 },
}

/// It has an inode and an offset.
///
/// # Safety
///
/// The offset should be accessed only when the inode is locked.
pub struct InodeFileType {
    pub ip: RcInode<DefaultFs>,
    // It should be accessed only when `ip` is locked.
    pub off: UnsafeCell<u32>,
}

/// It can be acquired when the inode of `InodeFileType` is locked. `ip` is the guard of the locked
/// inode. `off` is a mutable reference to the offset. Accessing `off` is guaranteed to be safe
/// since the inode is locked.
struct InodeFileTypeGuard<'a, FS: FileSystem> {
    ip: ManuallyDrop<InodeGuard<'a, FS>>,
    off: &'a mut u32,
}

pub struct File {
    pub typ: FileType,
    readable: bool,
    writable: bool,
}

pub type FileTable = ArrayArena<File, NFILE>;

/// map major device number to device functions.
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<fn(UVAddr, i32, &mut KernelCtx<'_, '_>) -> i32>,
    pub write: Option<fn(UVAddr, i32, &mut KernelCtx<'_, '_>) -> i32>,
}

/// A reference counted smart pointer to a `File`.
pub type RcFile = ArenaRc<FileTable>;

// Events for `select`
pub enum SelectEvent {
    Read,
    _Write,
    _Error,
}

pub enum SeekWhence {
    Set,
    Cur,
    End,
}

impl Default for FileType {
    fn default() -> Self {
        Self::None
    }
}

impl InodeFileType {
    fn lock(&self, ctx: &KernelCtx<'_, '_>) -> InodeFileTypeGuard<'_, DefaultFs> {
        let ip = self.ip.lock(ctx);
        // SAFETY: `ip` is locked and `off` can be exclusively accessed.
        let off = unsafe { &mut *self.off.get() };
        InodeFileTypeGuard {
            ip: ManuallyDrop::new(ip),
            off,
        }
    }
}

impl<FS: FileSystem> InodeFileTypeGuard<'_, FS> {
    fn free(mut self, ctx: &KernelCtx<'_, '_>) {
        let ip = unsafe { ManuallyDrop::take(&mut self.ip) };
        ip.free(ctx);
        core::mem::forget(self);
    }
}

impl<'a, FS: FileSystem> Deref for InodeFileTypeGuard<'a, FS> {
    type Target = InodeGuard<'a, FS>;

    fn deref(&self) -> &Self::Target {
        &self.ip
    }
}

impl<'a, FS: FileSystem> DerefMut for InodeFileTypeGuard<'a, FS> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ip
    }
}

impl<FS: FileSystem> Drop for InodeFileTypeGuard<'_, FS> {
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

    /// Repositions the file offset of the open file description
    /// associated with the file descriptor fd to the `n` according
    /// to the directive `option`.
    pub fn lseek(
        &self,
        n: i32,
        option: SeekWhence,
        ctx: &mut KernelCtx<'_, '_>,
    ) -> Result<usize, ()> {
        if !self.readable {
            return Err(());
        }

        if let FileType::Inode { inner } = &self.typ {
            let ip = inner.lock(ctx);
            let off = match option {
                SeekWhence::Set => n as u32,
                SeekWhence::Cur => *ip.off + n as u32,
                SeekWhence::End => {
                    let ip_inner = ip.deref_inner();
                    ip_inner.size + n as u32
                }
            };
            *ip.off = off;
            ip.free(ctx);
            Ok(off as usize)
        } else {
            Err(())
        }
    }

    /// Check file is ready for specified select event.
    /// It only supports pipe now.
    /// TODO: support other type of files
    pub fn is_ready(&self, event: SelectEvent) -> Result<bool, ()> {
        match event {
            SelectEvent::Read => {
                if !self.readable {
                    return Err(());
                }

                match &self.typ {
                    FileType::Pipe { pipe } => {
                        // pipe-empty
                        if pipe.is_ready(event) {
                            return Ok(true);
                        }
                    }
                    FileType::Inode { .. } => {
                        unimplemented!()
                    }
                    FileType::Device { .. } => unimplemented!(""),
                    FileType::None => panic!("Syscall::sys_select"),
                }
                Ok(false)
            }
            _ => {
                todo!("Select for write and error is not implemented yet")
            }
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

    fn finalize<'a, 'id: 'a>(&mut self, ctx: Self::Ctx<'a, 'id>) {
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
        ArrayArena::<File, NFILE>::new("FTABLE")
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
