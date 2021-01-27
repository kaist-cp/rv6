//! File-system system calls.
//! Mostly argument checking, since we don't trust
//! user code, and calls into file.c and fs.c.

#![allow(clippy::unit_arg)]

use crate::{
    fcntl::FcntlFlags,
    file::{FileType, RcFile},
    fs::{Dirent, FileName, FsTransaction, InodeGuard, InodeType, Path, RcInode, DIRENT_SIZE},
    kernel::{kernel, Kernel},
    ok_or,
    page::Page,
    param::{MAXARG, MAXPATH, NDEV, NOFILE},
    pipe::AllocatedPipe,
    proc::myproc,
    some_or,
    syscall::{argaddr, argint, argstr, fetchaddr, fetchstr},
    vm::{KVAddr, UVAddr, VAddr},
};

use arrayvec::ArrayVec;
use core::{cell::UnsafeCell, mem, slice};
use cstr_core::CStr;

impl RcFile<'static> {
    /// Allocate a file descriptor for the given file.
    /// Takes over file reference from caller on success.
    fn fdalloc(self) -> Result<i32, Self> {
        // TODO(https://github.com/kaist-cp/rv6/issues/354)
        // These two unsafe blocks need to be safe after we refactor myproc()
        let p = unsafe { myproc() };
        let mut data = unsafe { &mut *(*p).data.get() };
        for fd in 0..NOFILE {
            // user pointer to struct stat
            if data.open_files[fd].is_none() {
                data.open_files[fd] = Some(self);
                return Ok(fd as i32);
            }
        }
        Err(self)
    }
}

/// Fetch the nth word-sized system call argument as a file descriptor
/// and return both the descriptor and the corresponding struct file.
unsafe fn argfd(n: usize) -> Result<(i32, &'static RcFile<'static>), ()> {
    let fd = unsafe { argint(n)? };
    if fd < 0 || fd >= NOFILE as i32 {
        return Err(());
    }

    let f = some_or!(
        unsafe { &(*(*myproc()).data.get()).open_files[fd as usize] },
        return Err(())
    );

    Ok((fd, f))
}

/// Create an inode with given type.
/// Returns Ok(created inode, result of given function f) on success, Err(()) on error.
fn create<F, T>(
    path: &Path,
    typ: InodeType,
    tx: &FsTransaction<'_>,
    f: F,
) -> Result<(RcInode<'static>, T), ()>
where
    F: FnOnce(&mut InodeGuard<'_>) -> T,
{
    let (ptr, name) = path.nameiparent()?;
    let mut dp = ptr.lock();
    if let Ok((ptr2, _)) = dp.dirlookup(&name) {
        drop(dp);
        let mut ip = ptr2.lock();
        if typ == InodeType::File {
            match ip.deref_inner().typ {
                InodeType::File | InodeType::Device { .. } => {
                    let ret = f(&mut ip);
                    drop(ip);
                    return Ok((ptr2, ret));
                }
                _ => return Err(()),
            }
        }
        return Err(());
    }
    let ptr2 = unsafe { kernel().itable.alloc_inode(dp.dev, typ, tx) };
    let mut ip = ptr2.lock();
    ip.deref_inner_mut().nlink = 1;
    // It is safe to call update() because unique access to ip is guaranteed.
    unsafe { ip.update(tx) };

    // Create . and .. entries.
    if typ == InodeType::Dir {
        // for ".."
        dp.deref_inner_mut().nlink += 1;
        // It is safe to call update() because unique access to dp is guaranteed.
        unsafe { dp.update(tx) };

        // No ip->nlink++ for ".": avoid cyclic ref count.
        // It is safe because b"." does not contain any NUL characters.
        ip.dirlink(unsafe { FileName::from_bytes(b".") }, ip.inum, tx)
            // It is safe because b".." does not contain any NUL characters.
            .and_then(|_| ip.dirlink(unsafe { FileName::from_bytes(b"..") }, dp.inum, tx))
            .expect("create dots");
    }
    dp.dirlink(&name, ip.inum, tx).expect("create: dirlink");
    let ret = f(&mut ip);
    drop(ip);
    Ok((ptr2, ret))
}

impl Kernel {
    /// Create another name(newname) for the file oldname.
    /// Returns Ok(()) on success, Err(()) on error.
    fn link(&self, oldname: &CStr, newname: &CStr) -> Result<(), ()> {
        let tx = self.file_system.begin_transaction();
        let ptr = Path::new(oldname).namei()?;
        let mut ip = ptr.lock();
        if ip.deref_inner().typ == InodeType::Dir {
            return Err(());
        }
        ip.deref_inner_mut().nlink += 1;
        // It is safe to call update() because unique access to ip is guaranteed.
        unsafe { ip.update(&tx) };
        drop(ip);

        if let Ok((ptr2, name)) = Path::new(newname).nameiparent() {
            let mut dp = ptr2.lock();
            if dp.dev != ptr.dev || dp.dirlink(name, ptr.inum, &tx).is_err() {
            } else {
                return Ok(());
            }
        }

        let mut ip = ptr.lock();
        ip.deref_inner_mut().nlink -= 1;
        // It is safe to call update() because unique access to ip is guaranteed.
        unsafe { ip.update(&tx) };
        Err(())
    }

    /// Remove a file(filename).
    /// Returns Ok(()) on success, Err(()) on error.
    fn unlink(&self, filename: &CStr) -> Result<(), ()> {
        let mut de: Dirent = Default::default();
        let tx = self.file_system.begin_transaction();
        let (ptr, name) = Path::new(filename).nameiparent()?;
        let mut dp = ptr.lock();

        // Cannot unlink "." or "..".
        if !(name.as_bytes() == b"." || name.as_bytes() == b"..") {
            if let Ok((ptr2, off)) = dp.dirlookup(&name) {
                let mut ip = ptr2.lock();
                assert!(ip.deref_inner().nlink >= 1, "unlink: nlink < 1");

                if ip.deref_inner().typ != InodeType::Dir || ip.is_dir_empty() {
                    let bytes_write = dp.write(
                        KVAddr::new(&mut de as *mut Dirent as usize),
                        off,
                        DIRENT_SIZE as u32,
                        &tx,
                    );
                    assert_eq!(bytes_write, Ok(DIRENT_SIZE), "unlink: writei");
                    if ip.deref_inner().typ == InodeType::Dir {
                        dp.deref_inner_mut().nlink -= 1;
                        // It is safe to call update() because unique access to dp is guaranteed.
                        unsafe { dp.update(&tx) };
                    }
                    drop(dp);
                    drop(ptr);
                    ip.deref_inner_mut().nlink -= 1;
                    // It is safe to call update() because unique access to ip is guaranteed.
                    unsafe { ip.update(&tx) };
                    return Ok(());
                }
            }
        }

        Err(())
    }

    /// Open a file; omode indicate read/write.
    /// Returns Ok(file descriptor) on success, Err(()) on error.
    fn open(&'static self, name: &Path, omode: FcntlFlags) -> Result<usize, ()> {
        let tx = self.file_system.begin_transaction();

        let (ip, typ) = if omode.contains(FcntlFlags::O_CREATE) {
            create(name, InodeType::File, &tx, |ip| ip.deref_inner().typ)?
        } else {
            let ptr = name.namei()?;
            let ip = ptr.lock();
            let typ = ip.deref_inner().typ;

            if typ == InodeType::Dir && omode != FcntlFlags::O_RDONLY {
                return Err(());
            }
            drop(ip);
            (ptr, typ)
        };

        let filetype = match typ {
            InodeType::Device { major, .. } => {
                if major as usize >= NDEV {
                    return Err(());
                };
                FileType::Device { ip, major }
            }
            _ => FileType::Inode {
                ip,
                off: UnsafeCell::new(0),
            },
        };

        let f = self.ftable.alloc_file(
            filetype,
            !omode.intersects(FcntlFlags::O_WRONLY),
            omode.intersects(FcntlFlags::O_WRONLY | FcntlFlags::O_RDWR),
        )?;

        if omode.contains(FcntlFlags::O_TRUNC) && typ == InodeType::File {
            match &f.typ {
                // It is safe to call itrunc because ip.lock() is held
                FileType::Device { ip, .. } | FileType::Inode { ip, .. } => unsafe {
                    ip.lock().itrunc(&tx)
                },
                _ => panic!("sys_open : Not reach"),
            };
        }
        let fd = f.fdalloc().map_err(|_| ())?;
        Ok(fd as usize)
    }

    /// Create a new directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn mkdir(&self, dirname: &CStr) -> Result<(), ()> {
        let tx = self.file_system.begin_transaction();
        create(Path::new(dirname), InodeType::Dir, &tx, |_| ())?;
        Ok(())
    }

    /// Create a device file.
    /// Returns Ok(()) on success, Err(()) on error.
    fn mknod(&self, filename: &CStr, major: u16, minor: u16) -> Result<(), ()> {
        let tx = self.file_system.begin_transaction();
        create(
            Path::new(filename),
            InodeType::Device { major, minor },
            &tx,
            |_| (),
        )?;
        Ok(())
    }

    /// Change the current directory.
    /// Returns Ok(()) on success, Err(()) on error.
    fn chdir(&self, dirname: &CStr) -> Result<(), ()> {
        // TODO(https://github.com/kaist-cp/rv6/issues/354)
        // These two unsafe blocks need to be safe after we refactor myproc()
        let p = unsafe { myproc() };
        let mut data = unsafe { &mut *(*p).data.get() };
        // TODO(https://github.com/kaist-cp/rv6/issues/290)
        // The method namei can drop inodes. If namei succeeds, its return
        // value, ptr, will be dropped when this method returns. Deallocation
        // of an inode may cause disk write operations, so we must begin a
        // transaction here.
        let _tx = self.file_system.begin_transaction();
        let ptr = Path::new(dirname).namei()?;
        let ip = ptr.lock();
        if ip.deref_inner().typ != InodeType::Dir {
            return Err(());
        }
        drop(ip);
        data.cwd = Some(ptr);
        Ok(())
    }

    /// Create a pipe, put read/write file descriptors in fd0 and fd1.
    /// Returns Ok(()) on success, Err(()) on error.
    fn pipe(&self, fdarray: UVAddr) -> Result<(), ()> {
        // TODO(https://github.com/kaist-cp/rv6/issues/354)
        // These two unsafe blocks need to be safe after we refactor myproc()
        let p = unsafe { myproc() };
        let mut data = unsafe { &mut *(*p).data.get() };
        let (pipereader, pipewriter) = AllocatedPipe::alloc()?;

        let mut fd0 = pipereader.fdalloc().map_err(|_| ())?;
        let mut fd1 = pipewriter
            .fdalloc()
            .map_err(|_| data.open_files[fd0 as usize] = None)?;

        // It is safe because fdarray, fd0 is valid.
        if unsafe {
            data.memory.copy_out(
                fdarray,
                slice::from_raw_parts_mut(&mut fd0 as *mut i32 as *mut u8, mem::size_of::<i32>()),
            )
        }
        .is_err()
            // It is safe because fdarray, fd1 is valid.
            || unsafe {
                data.memory.copy_out(
                    UVAddr::new(fdarray.into_usize().wrapping_add(mem::size_of::<i32>())),
                    slice::from_raw_parts_mut(
                        &mut fd1 as *mut i32 as *mut u8,
                        mem::size_of::<i32>(),
                    ),
                )
            }
            .is_err()
        {
            data.open_files[fd0 as usize] = None;
            data.open_files[fd1 as usize] = None;
            return Err(());
        }
        Ok(())
    }
}

impl Kernel {
    /// Return a new file descriptor referring to the same file as given fd.
    /// Returns Ok(new file descriptor) on success, Err(()) on error.
    pub unsafe fn sys_dup(&self) -> Result<usize, ()> {
        let (_, f) = unsafe { argfd(0)? };
        let newfile = f.clone();
        let fd = newfile.fdalloc().map_err(|_| ())?;
        Ok(fd as usize)
    }

    /// Read n bytes into buf.
    /// Returns Ok(number read) on success, Err(()) on error.
    pub unsafe fn sys_read(&self) -> Result<usize, ()> {
        let (_, f) = unsafe { argfd(0)? };
        let n = unsafe { argint(2)? };
        let p = unsafe { argaddr(1)? };
        unsafe { f.read(UVAddr::new(p), n) }
    }

    /// Write n bytes from buf to given file descriptor fd.
    /// Returns Ok(n) on success, Err(()) on error.
    pub unsafe fn sys_write(&self) -> Result<usize, ()> {
        let (_, f) = unsafe { argfd(0)? };
        let n = unsafe { argint(2)? };
        let p = unsafe { argaddr(1)? };
        unsafe { f.write(UVAddr::new(p), n) }
    }

    /// Release open file fd.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_close(&self) -> Result<usize, ()> {
        let (fd, _) = unsafe { argfd(0)? };
        // TODO(https://github.com/kaist-cp/rv6/issues/354)
        // This line should be safe after we refactor myporc()
        unsafe { (*(*myproc()).data.get()).open_files[fd as usize] = None };
        Ok(0)
    }

    /// Place info about an open file into struct stat.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_fstat(&self) -> Result<usize, ()> {
        let (_, f) = unsafe { argfd(0)? };
        // user pointer to struct stat
        let st = unsafe { argaddr(1)? };
        unsafe { f.stat(UVAddr::new(st))? };
        Ok(0)
    }

    /// Create the path new as a link to the same inode as old.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_link(&self) -> Result<usize, ()> {
        let mut new: [u8; MAXPATH] = [0; MAXPATH];
        let mut old: [u8; MAXPATH] = [0; MAXPATH];
        let old = unsafe { argstr(0, &mut old)? };
        let new = unsafe { argstr(1, &mut new)? };
        self.link(old, new)?;
        Ok(0)
    }

    /// Remove a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_unlink(&self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = unsafe { argstr(0, &mut path)? };
        self.unlink(path)?;
        Ok(0)
    }

    /// Open a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_open(&'static self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = unsafe { argstr(0, &mut path)? };
        let path = Path::new(path);
        let omode = unsafe { argint(1)? };
        let omode = FcntlFlags::from_bits_truncate(omode);
        self.open(path, omode)
    }

    /// Create a new directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_mkdir(&self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = unsafe { argstr(0, &mut path)? };
        self.mkdir(path)?;
        Ok(0)
    }

    /// Create a new directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_mknod(&self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = unsafe { argstr(0, &mut path)? };
        let major = unsafe { argint(1)? } as u16;
        let minor = unsafe { argint(2)? } as u16;
        self.mknod(path, major, minor)?;
        Ok(0)
    }

    /// Change the current directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_chdir(&self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = unsafe { argstr(0, &mut path)? };
        self.chdir(path)?;
        Ok(0)
    }

    /// Load a file and execute it with arguments.
    /// Returns Ok(argc argument to user main) on success, Err(()) on error.
    pub unsafe fn sys_exec(&self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let mut args = ArrayVec::<[Page; MAXARG]>::new();
        let path = unsafe { argstr(0, &mut path)? };
        let uargv = unsafe { argaddr(1)? };

        let mut success = false;
        for i in 0..MAXARG {
            let uarg = ok_or!(
                unsafe { fetchaddr(UVAddr::new(uargv + mem::size_of::<usize>() * i)) },
                break
            );

            if uarg == 0 {
                success = true;
                break;
            }

            let mut page = some_or!(self.alloc(), break);
            if unsafe { fetchstr(UVAddr::new(uarg), &mut page[..]) }.is_err() {
                self.free(page);
                break;
            }
            args.push(page);
        }

        let ret = if success {
            self.exec(Path::new(path), &args)
        } else {
            Err(())
        };

        for page in args.drain(..) {
            self.free(page);
        }

        ret
    }

    /// Create a pipe.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_pipe(&self) -> Result<usize, ()> {
        // user pointer to array of two integers
        let fdarray = UVAddr::new(unsafe { argaddr(0)? });
        self.pipe(fdarray)?;
        Ok(0)
    }
}
