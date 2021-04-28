use crate::{arch::poweroff, proc::KernelCtx};

impl KernelCtx<'_, '_> {
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
        let kmem = &self.kernel().kmem;
        self.proc_mut().memory_mut().resize(n, kmem)
    }

    /// Pause for n clock ticks.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_sleep(&self) -> Result<usize, ()> {
        let n = self.proc().argint(0)?;
        let mut ticks = self.kernel().ticks().lock();
        let ticks0 = *ticks;
        while ticks.wrapping_sub(ticks0) < n as u32 {
            if self.proc().killed() {
                return Err(());
            }
            ticks.sleep();
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

    /// Shutdowns this machine, discarding all unsaved data. No return.
    pub fn sys_poweroff(&self) -> Result<usize, ()> {
        let exitcode = self.proc().argint(0)?;
        poweroff::machine_poweroff(exitcode as _);
    }
}
