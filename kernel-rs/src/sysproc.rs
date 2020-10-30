use crate::{
    kernel::kernel,
    ok_or, poweroff,
    proc::{myproc, resizeproc},
    syscall::{argaddr, argint},
};

pub unsafe fn sys_exit() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    kernel().procs.exit_current(n);

    panic!("sys_exit: not reached");
}

pub unsafe fn sys_getpid() -> usize {
    (*myproc()).pid() as _
}

pub unsafe fn sys_fork() -> usize {
    kernel().procs.fork() as _
}

pub unsafe fn sys_wait() -> usize {
    let p = ok_or!(argaddr(0), return usize::MAX);
    kernel().procs.wait(p) as _
}

pub unsafe fn sys_sbrk() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    let addr: i32 = (*(*myproc()).data.get()).sz as i32;
    if resizeproc(n) < 0 {
        return usize::MAX;
    }
    addr as usize
}

pub unsafe fn sys_sleep() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    let mut ticks = kernel().ticks.lock();
    let ticks0 = *ticks;
    while ticks.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed() {
            return usize::MAX;
        }
        ticks.sleep();
    }
    0
}

pub unsafe fn sys_kill() -> usize {
    let pid = ok_or!(argint(0), return usize::MAX);
    kernel().procs.kill(pid) as usize
}

/// return how many clock tick interrupts have occurred
/// since start.
pub unsafe fn sys_uptime() -> usize {
    *kernel().ticks.lock() as usize
}

pub unsafe fn sys_poweroff() -> usize {
    let exitcode = ok_or!(argint(0), return usize::MAX);
    poweroff::machine_poweroff(exitcode as _);
}
