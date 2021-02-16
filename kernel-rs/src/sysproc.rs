use crate::{
    kernel::Kernel,
    poweroff,
    proc::CurrentProc,
    syscall::{argaddr, argint},
};

impl Kernel {
    /// Terminate the current process; status reported to wait(). No return.
    pub unsafe fn sys_exit(&self, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
        let n = argint(0, proc)?;
        unsafe { self.procs.exit_current(n, proc) };
    }

    /// Return the current process’s PID.
    pub fn sys_getpid(&self, proc: &CurrentProc<'_>) -> Result<usize, ()> {
        Ok(proc.pid() as _)
    }

    /// Create a process.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub unsafe fn sys_fork(&self, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
        Ok(unsafe { self.procs.fork(proc) }? as _)
    }

    /// Wait for a child to exit.
    /// Returns Ok(child’s PID) on success, Err(()) on error.
    pub unsafe fn sys_wait(&self, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
        let p = argaddr(0, proc)?;
        Ok(unsafe { self.procs.wait(p.into(), proc) }? as _)
    }

    /// Grow process’s memory by n bytes.
    /// Returns Ok(start of new memory) on success, Err(()) on error.
    pub fn sys_sbrk(&self, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
        let n = argint(0, proc)?;
        proc.deref_mut_data().memory.resize(n)
    }

    /// Pause for n clock ticks.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_sleep(&self, proc: &CurrentProc<'_>) -> Result<usize, ()> {
        let n = argint(0, proc)?;
        let mut ticks = self.ticks.lock();
        let ticks0 = *ticks;
        while ticks.wrapping_sub(ticks0) < n as u32 {
            if proc.killed() {
                return Err(());
            }
            ticks.sleep();
        }
        Ok(0)
    }

    /// Terminate process PID.
    /// Returns Ok(0) on success, Err(()) on error.
    pub fn sys_kill(&self, proc: &CurrentProc<'_>) -> Result<usize, ()> {
        let pid = argint(0, proc)?;
        self.procs.kill(pid)?;
        Ok(0)
    }

    /// Return how many clock tick interrupts have occurred
    /// since start.
    pub fn sys_uptime(&self, _proc: &CurrentProc<'_>) -> Result<usize, ()> {
        Ok(*self.ticks.lock() as usize)
    }

    /// Shutdowns this machine, discarding all unsaved data. No return.
    pub fn sys_poweroff(&self, proc: &CurrentProc<'_>) -> Result<usize, ()> {
        let exitcode = argint(0, proc)?;
        poweroff::machine_poweroff(exitcode as _);
    }
}
