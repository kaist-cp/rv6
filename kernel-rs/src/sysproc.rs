use crate::{
    kernel::Kernel,
    poweroff,
    proc::{myproc, resizeproc},
    syscall::{argaddr, argint},
    vm::{UVAddr, VAddr},
};

impl Kernel {
    /// Terminate the current process; status reported to wait(). No return.
    pub unsafe fn sys_exit(&self) -> Result<usize, ()> {
        let n = argint(0)?;
        self.procs.exit_current(n);
    }

    /// Return the current process’s PID.
    pub unsafe fn sys_getpid(&self) -> Result<usize, ()> {
        Ok((*myproc()).pid() as _)
    }

    /// Create a process.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub unsafe fn sys_fork(&self) -> Result<usize, ()> {
        Ok(self.procs.fork()? as _)
    }

    /// Wait for a child to exit.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub unsafe fn sys_wait(&self) -> Result<usize, ()> {
        let p = argaddr(0)?;
        Ok(self.procs.wait(UVAddr::new(p))? as _)
    }

    /// Grow process’s memory by n bytes.
    /// Returns Ok(start of new memory) on success, Err(()) on error.
    pub unsafe fn sys_sbrk(&self) -> Result<usize, ()> {
        let n = argint(0)?;
        let addr = (*(*myproc()).data.get()).sz as i32;
        if resizeproc(n) < 0 {
            return Err(());
        }
        Ok(addr as usize)
    }

    /// Pause for n clock ticks.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_sleep(&self) -> Result<usize, ()> {
        let n = argint(0)?;
        let mut ticks = self.ticks.lock();
        let ticks0 = *ticks;
        while ticks.wrapping_sub(ticks0) < n as u32 {
            if (*myproc()).killed() {
                return Err(());
            }
            ticks.sleep();
        }
        Ok(0)
    }

    /// Terminate process PID.
    /// Returns Ok(0) on success, Err(()) on error.
    pub unsafe fn sys_kill(&self) -> Result<usize, ()> {
        let pid = argint(0)?;
        self.procs.kill(pid)?;
        Ok(0)
    }

    /// Return how many clock tick interrupts have occurred
    /// since start.
    pub unsafe fn sys_uptime(&self) -> Result<usize, ()> {
        Ok(*self.ticks.lock() as usize)
    }

    /// Shutdowns this machine, discarding all unsaved data. No return.
    pub unsafe fn sys_poweroff(&self) -> Result<usize, ()> {
        let exitcode = argint(0)?;
        poweroff::machine_poweroff(exitcode as _);
    }
}
