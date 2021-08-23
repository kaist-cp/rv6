//! System calls.
//!
//! Mostly argument checking, since we don't trust user code, and calls into inner methods.

#![allow(clippy::unit_arg)]

use core::{mem, str};

use arrayvec::ArrayVec;
use cstr_core::CStr;

use crate::{
    addr::{Addr, UVAddr},
    arch::interface::TimeManager,
    arch::poweroff,
    arch::timer::Timer,
    file::RcFile,
    fs::{FcntlFlags, FileSystem, FileSystemExt, InodeType, Path},
    file::{FileType, RcFile, SelectEvent, SeekWhence},
    file::{FileType, RcFile, SeekWhence, SelectEvent},
    arch::interface::{PowerOff, TimeManager},
    arch::interface::{PowerOff, TimeManager, TrapFrameManager},
    arch::TargetArch,
    file::{RcFile, SeekWhence, SelectEvent},
    fs::{FcntlFlags, FileSystem, InodeType, Path},
    hal::hal,
    ok_or,
    page::{Page, PGSIZE},
    param::{MAXARG, MAXPATH},
    proc::{CurrentProc, KernelCtx},
    some_or,
};

impl CurrentProc<'_, '_> {
    /// Fetch the usize at addr from the current process.
    /// Returns Ok(fetched integer) on success, Err(()) on error.
    pub fn fetchaddr(&mut self, addr: UVAddr) -> Result<usize, ()> {
        let mut ip = 0;
        let sz = mem::size_of::<usize>();
        if addr.into_usize() >= self.memory().size()
            || addr.into_usize() + sz > self.memory().size()
        {
            return Err(());
        }
        // SAFETY: usize does not have any internal structure.
        unsafe { self.memory_mut().copy_in(&mut ip, addr) }?;
        Ok(ip)
    }

    /// Fetch the nul-terminated string at addr from the current process.
    /// Returns reference to the string in the buffer.
    pub fn fetchstr<'a>(&mut self, addr: UVAddr, buf: &'a mut [u8]) -> Result<&'a CStr, ()> {
        self.memory_mut().copy_in_str(buf, addr)?;

        // SAFETY: buf contains '\0' as copy_in_str has succeeded.
        Ok(unsafe { CStr::from_ptr(buf.as_ptr()) })
    }

    fn argraw(&self, n: usize) -> usize {
        self.trap_frame().get_param_reg(n.into())
    }

    /// Fetch the nth 32-bit system call argument.
    pub fn argint(&self, n: usize) -> Result<i32, ()> {
        Ok(self.argraw(n) as i32)
    }

    /// Retrieve an argument as a pointer.
    /// Doesn't check for legality, since
    /// copyin/copyout will do that.
    pub fn argaddr(&self, n: usize) -> Result<usize, ()> {
        Ok(self.argraw(n))
    }

    /// Fetch the nth word-sized system call argument as a null-terminated string.
    /// Copies into buf, at most max.
    /// Returns reference to the string in the buffer.
    pub fn argstr<'a>(&mut self, n: usize, buf: &'a mut [u8]) -> Result<&'a CStr, ()> {
        let addr = self.argaddr(n)?;
        self.fetchstr(addr.into(), buf)
    }

    /// Fetch the nth word-sized system call argument as a file descriptor
    /// and return both the descriptor and the corresponding struct file.
    fn argfd(&self, n: usize) -> Result<(i32, &RcFile), ()> {
        let fd = self.argint(n)?;
        let f = self
            .deref_data()
            .open_files
            .get(fd as usize)
            .ok_or(())?
            .as_ref()
            .ok_or(())?;
        Ok((fd, f))
    }
}

impl KernelCtx<'_, '_> {
    pub fn syscall(&mut self, num: i32) -> Result<usize, ()> {
        match num {
            1 => self.sys_fork(),
            2 => self.sys_exit(),
            3 => self.sys_wait(),
            4 => self.sys_pipe(),
            5 => self.sys_read(),
            6 => self.sys_kill(),
            7 => self.sys_exec(),
            8 => self.sys_fstat(),
            9 => self.sys_chdir(),
            10 => self.sys_dup(),
            11 => self.sys_getpid(),
            12 => self.sys_sbrk(),
            13 => self.sys_sleep(),
            14 => self.sys_uptime(),
            15 => self.sys_open(),
            16 => self.sys_write(),
            17 => self.sys_mknod(),
            18 => self.sys_unlink(),
            19 => self.sys_link(),
            20 => self.sys_mkdir(),
            21 => self.sys_close(),
            22 => self.sys_poweroff(),
            23 => self.sys_select(),
            24 => self.sys_getpagesize(),
            25 => self.sys_waitpid(),
            26 => self.sys_getppid(),
            27 => self.sys_lseek(),
            28 => self.sys_uptime_as_micro(),
            _ => {
                self.kernel().as_ref().write_fmt(format_args!(
                    "{} {}: unknown sys call {}",
                    self.proc().pid(),
                    str::from_utf8(&self.proc().deref_data().name).unwrap_or("???"),
                    num
                ));
                Err(())
            }
        }
    }

    /// Terminate the current process; status reported to wait(). No return.
    pub fn sys_exit(&mut self) -> Result<usize, ()> {
        let n = self.proc().argint(0)?;
        self.kernel().procs().exit_current(n, self);
    }

    /// Create a process.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub fn sys_fork(&mut self) -> Result<usize, ()> {
        Ok(self.kernel().procs().fork(self)? as _)
    }

    /// Wait for a child to exit.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub fn sys_wait(&mut self) -> Result<usize, ()> {
        let p = self.proc().argaddr(0)?;
        Ok(self.kernel().procs().wait(p.into(), self)? as _)
    }

    /// Return the current process’s PID.
    pub fn sys_getpid(&self) -> Result<usize, ()> {
        Ok(self.proc().pid() as _)
    }

    /// Grow process’s memory by n bytes.
    /// Returns Ok(start of new memory) on success, Err(()) on error.
    pub fn sys_sbrk(&mut self) -> Result<usize, ()> {
        let n = self.proc().argint(0)?;
        self.proc_mut().memory_mut().resize(n, hal().kmem())
    }

    /// Pause for n clock ticks.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_sleep(&self) -> Result<usize, ()> {
        let n = self.proc().argint(0)?;
        assert!(n >= 0);

        let mut ticks = self.kernel().ticks().lock();
        let ticks0 = *ticks;
        while ticks.wrapping_sub(ticks0) < n as u32 {
            if self.proc().killed() {
                return Err(());
            }
            ticks.sleep(self);
        }
        Ok(0)
    }

    /// Terminate process PID.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_kill(&self) -> Result<usize, ()> {
        let pid = self.proc().argint(0)?;
        self.kernel().procs().kill(pid)?;
        Ok(0)
    }

    /// Return how many clock tick interrupts have occurred
    /// since start.
    pub fn sys_uptime(&self) -> Result<usize, ()> {
        Ok(*self.kernel().ticks().lock() as usize)
    }

    /// Return how much time has passed since start,
    /// in microseconds.
    pub fn sys_uptime_as_micro(&self) -> Result<usize, ()> {
        TargetArch::uptime_as_micro()
    }

    /// Shutdowns this machine, discarding all unsaved data. No return.
    pub fn sys_poweroff(&self) -> Result<usize, ()> {
        let exitcode = self.proc().argint(0)?;
        TargetArch::machine_poweroff(exitcode as _);
    }

    /// Return a new file descriptor referring to the same file as given fd.
    /// Returns Ok(new file descriptor) on success, Err(()) on error.
    pub fn sys_dup(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let newfile = f.clone();
        let fd = newfile.fdalloc(self)?;
        Ok(fd as usize)
    }

    /// Read n bytes into buf.
    /// Returns Ok(number read) on success, Err(()) on error.
    pub fn sys_read(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let n = self.proc().argint(2)?;
        let p = self.proc().argaddr(1)?;
        // SAFETY: read will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).read(p.into(), n, self) }
    }

    /// Write n bytes from buf to given file descriptor fd.
    /// Returns Ok(n) on success, Err(()) on error.
    pub fn sys_write(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let n = self.proc().argint(2)?;
        let p = self.proc().argaddr(1)?;
        // SAFETY: write will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).write(p.into(), n, self) }
    }

    /// Release open file fd.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_close(&mut self) -> Result<usize, ()> {
        let (fd, _) = self.proc().argfd(0)?;
        if let Some(f) = self.proc_mut().deref_mut_data().open_files[fd as usize].take() {
            f.free(self);
        }
        Ok(0)
    }

    /// Place info about an open file into struct stat.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_fstat(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        // user pointer to struct stat
        let st = self.proc().argaddr(1)?;
        // SAFETY: stat will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).stat(st.into(), self) }?;
        Ok(0)
    }

    /// Create the path new as a link to the same inode as old.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_link(&mut self) -> Result<usize, ()> {
        let mut new: [u8; MAXPATH] = [0; MAXPATH];
        let mut old: [u8; MAXPATH] = [0; MAXPATH];
        let old = Path::new(self.proc_mut().argstr(0, &mut old)?);
        let new = Path::new(self.proc_mut().argstr(1, &mut new)?);
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = try {
            let inode = self.kernel().fs().namei(old, &tx, self)?;
            let _ = self.kernel().fs().link(inode, new, &tx, self)?;
            0
        };
        tx.end(self);
        res
    }

    /// Remove a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_unlink(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = self.kernel().fs().unlink(path, &tx, self).map(|_| 0);
        tx.end(self);
        res
    }

    /// Open a file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_open(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let omode = self.proc().argint(1)?;
        let omode = FcntlFlags::from_bits_truncate(omode);
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = self.kernel().fs().open(path, omode, &tx, self);
        tx.end(self);
        res
    }

    /// Create a new directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_mkdir(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = self
            .kernel()
            .fs()
            .create(path, InodeType::Dir, &tx, self, |_| ())
            .map(|(ptr, _)| {
                ptr.free((&tx, self));
                0
            });
        tx.end(self);
        res
    }

    /// Create a new device file.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_mknod(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let major = self.proc().argint(1)? as u16;
        let minor = self.proc().argint(2)? as u16;
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = self
            .kernel()
            .fs()
            .create(path, InodeType::Device { major, minor }, &tx, self, |_| ())
            .map(|(ptr, _)| {
                ptr.free((&tx, self));
                0
            });
        tx.end(self);
        res
    }

    /// Change the current directory.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_chdir(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let tx = self.kernel().fs().as_pin().get_ref().begin_tx(self);
        let res = try {
            let inode = self.kernel().fs().namei(path, &tx, self)?;
            let _ = self.kernel().fs().chdir(inode, &tx, self)?;
            0
        };
        tx.end(self);
        res
    }

    /// Load a file and execute it with arguments.
    /// Returns Ok(argc argument to user main) on success, Err(()) on error.
    pub fn sys_exec(&mut self) -> Result<usize, ()> {
        let mut path: [u8; MAXPATH] = [0; MAXPATH];
        let mut args = ArrayVec::<Page, MAXARG>::new();
        let path = Path::new(self.proc_mut().argstr(0, &mut path)?);
        let uargv = self.proc().argaddr(1)?;
        let allocator = hal().kmem();

        let mut success = false;
        for i in 0..MAXARG {
            let uarg = ok_or!(
                self.proc_mut()
                    .fetchaddr((uargv + mem::size_of::<usize>() * i).into()),
                break
            );

            if uarg == 0 {
                success = true;
                break;
            }

            let mut page = some_or!(allocator.alloc(), break);
            if self
                .proc_mut()
                .fetchstr(uarg.into(), &mut page[..])
                .is_err()
            {
                allocator.free(page);
                break;
            }
            args.push(page);
        }

        let ret = if success {
            self.exec(path, &args)
        } else {
            Err(())
        };

        for page in args.drain(..) {
            allocator.free(page);
        }

        ret
    }

    /// Create a pipe.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_pipe(&mut self) -> Result<usize, ()> {
        // user pointer to array of two integers
        let fdarray = self.proc().argaddr(0)?.into();
        self.pipe(fdarray)?;
        Ok(0)
    }

    /* Check the first NFDS descriptors each in READFDS (if not NULL) for read
    readiness, in WRITEFDS (if not NULL) for write readiness, and in EXCEPTFDS
    (if not NULL) for exceptional conditions.  If TIMEOUT is not NULL, time out
    after waiting the interval specified therein.  Returns the number of ready
    descriptors, or -1 for errors.
    Returns Ok(number of ready descriptors) on success, Err(()) on error.  */
    pub fn sys_select(&mut self) -> Result<usize, ()> {
        let nfds = self.proc().argint(0)?;
        let read_fds = self.proc().argaddr(1)?;
        let write_fds = self.proc().argaddr(2)?;
        let err_fds = self.proc().argaddr(3)?;

        let n_ticks = self.proc().argint(4)?;

        let ticks = self.kernel().ticks().lock();
        let ticks0 = *ticks;
        drop(ticks);

        let mut rfds = [0u8; 1024 / 8];
        let mut wfds = [0u8; 1024 / 8];
        let mut efds = [0u8; 1024 / 8];

        if read_fds != 0 {
            // SAFETY: `read_fds` is a valid user space address given by a user.
            unsafe {
                self.proc_mut()
                    .memory_mut()
                    .copy_in(&mut rfds, read_fds.into())
            }?;
        }

        if write_fds != 0 {
            unimplemented!("Handling for write fd set has not been implemented yet");
        }

        if err_fds != 0 {
            unimplemented!("Handling for exceptional fd set has not been implemented yet");
        }

        // the number of fds that are ready
        let mut ready_cnt = 0;

        let events = [SelectEvent::Read, SelectEvent::Write, SelectEvent::Error];
        let fds = [&mut rfds, &mut wfds, &mut efds];

        loop {
            // check fds
            for i in 0..3 {
                let event = events[i];

                for fd in 0..nfds + 1 {
                    let idx = (fd / 8) as usize;
                    let mask = 1 << (fd % 8);

                    if fds[i][idx] & mask != 0 {
                        let f = self
                            .proc()
                            .deref_data()
                            .open_files
                            .get(fd as usize)
                            .ok_or(())?
                            .as_ref()
                            .ok_or(())?;
                        // SAFETY: `is_ready` will not access proc's open_files.
                        if unsafe { (*(f as *const RcFile)).is_ready(event)? } {
                            ready_cnt += 1;
                        } else {
                            // If the fd is not ready, clear the bit.
                            fds[i][idx] &= !mask;
                        }
                    }
                }
            }

            if ready_cnt > 0 {
                break;
            }

            // check timeout
            let ticks = self.kernel().ticks().lock();
            if ticks.wrapping_sub(ticks0) >= n_ticks as u32 {
                for idx in 0..(nfds + 1) / 8 + 1 {
                    rfds[idx as usize] = 0;
                }
                break;
            }
            drop(ticks);
        }

        if read_fds != 0 {
            self.proc_mut()
                .memory_mut()
                .copy_out(read_fds.into(), &rfds)?;
        }

        if write_fds != 0 {
            self.proc_mut()
                .memory_mut()
                .copy_out(write_fds.into(), &wfds)?;
        }

        if err_fds != 0 {
            self.proc_mut()
                .memory_mut()
                .copy_out(err_fds.into(), &efds)?;
        }

        Ok(ready_cnt)
    }

    pub fn sys_getpagesize(&mut self) -> Result<usize, ()> {
        Ok(PGSIZE)
    }

    pub fn sys_waitpid(&mut self) -> Result<usize, ()> {
        let pid = self.proc().argint(0)?;
        let stat = self.proc().argaddr(1)?;
        Ok(self.kernel().procs().waitpid(pid, stat.into(), self)? as _)
    }

    pub fn sys_getppid(&mut self) -> Result<usize, ()> {
        Ok(self.kernel().procs().get_parent_pid(self) as _)
    }

    pub fn sys_lseek(&mut self) -> Result<usize, ()> {
        let (_, f) = self.proc().argfd(0)?;
        let offset = self.proc().argint(1)?;
        let whence = self.proc().argint(2)?;

        let whence = match whence {
            0 => SeekWhence::Set,
            1 => SeekWhence::Cur,
            2 => SeekWhence::End,
            _ => return Err(()),
        };
        // SAFETY: `lseek` will not access proc's open_files.
        unsafe { (*(f as *const RcFile)).lseek(offset, whence, self) }
    }
}
