use core::{mem, str};

use cstr_core::CStr;

use crate::{
    arch::addr::{Addr, UVAddr},
    println,
    proc::{CurrentProc, KernelCtx},
};

mod file;
mod proc;

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
            _ => {
                println!(
                    "{} {}: unknown sys call {}",
                    self.proc().pid(),
                    str::from_utf8(&self.proc().deref_data().name).unwrap_or("???"),
                    num
                );
                Err(())
            }
        }
    }
}

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
        match n {
            0 => self.trap_frame().a0,
            1 => self.trap_frame().a1,
            2 => self.trap_frame().a2,
            3 => self.trap_frame().a3,
            4 => self.trap_frame().a4,
            5 => self.trap_frame().a5,
            _ => panic!("argraw"),
        }
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
}
