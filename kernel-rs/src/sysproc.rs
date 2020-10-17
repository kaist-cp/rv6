use crate::{
    ok_or, poweroff,
    proc::{myproc, resizeproc, PROCSYS},
    syscall::{argaddr, argint},
    trap::TICKS,
};

pub unsafe fn sys_exit() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    PROCSYS.exit_current(n);

    panic!("sys_exit: not reached");
}

pub unsafe fn sys_getpid() -> usize {
    (*myproc()).pid as _
}

pub unsafe fn sys_fork() -> usize {
    PROCSYS.fork() as _
}

pub unsafe fn sys_wait() -> usize {
    let p = ok_or!(argaddr(0), return usize::MAX);
    PROCSYS.wait(p) as _
}

pub unsafe fn sys_sbrk() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    let addr: i32 = (*myproc()).sz as i32;
    if resizeproc(n) < 0 {
        return usize::MAX;
    }
    addr as usize
}

pub unsafe fn sys_sleep() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    let mut ticks = TICKS.lock();
    let ticks0 = *ticks;
    while ticks.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed {
            return usize::MAX;
        }
        ticks.sleep();
    }
    0
}

pub unsafe fn sys_kill() -> usize {
    let pid = ok_or!(argint(0), return usize::MAX);
    PROCSYS.kill(pid) as usize
}

/// return how many clock tick interrupts have occurred
/// since start.
pub unsafe fn sys_uptime() -> usize {
    *TICKS.lock() as usize
}

pub unsafe fn sys_poweroff() -> usize {
    let exitcode = ok_or!(argint(0), return usize::MAX);
    poweroff::machine_poweroff(exitcode as _);
}
