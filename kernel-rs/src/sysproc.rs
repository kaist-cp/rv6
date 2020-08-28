use crate::{
    libc, ok_or,
    proc::{exit, fork, kill, myproc, resizeproc, sleep, wait},
    syscall::{argaddr, argint},
    trap::{TICKS, TICKSLOCK},
};

pub unsafe fn sys_exit() -> usize {
    let n = ok_or!(argint(0), return usize::MAX);
    exit(n);

    panic!("sys_exit: not reached");
}

pub unsafe fn sys_getpid() -> usize {
    (*myproc()).pid as _
}

pub unsafe fn sys_fork() -> usize {
    fork() as _
}

pub unsafe fn sys_wait() -> usize {
    let p = ok_or!(argaddr(0), return usize::MAX);
    wait(p) as _
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
    TICKSLOCK.acquire();
    let ticks0 = TICKS;
    while TICKS.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed != 0 {
            TICKSLOCK.release();
            return usize::MAX;
        }
        sleep(&mut TICKS as *mut u32 as *mut libc::CVoid, &mut TICKSLOCK);
    }
    TICKSLOCK.release();
    0
}

pub unsafe fn sys_kill() -> usize {
    let pid = ok_or!(argint(0), return usize::MAX);
    kill(pid) as usize
}

/// return how many clock tick interrupts have occurred
/// since start.
pub unsafe fn sys_uptime() -> usize {
    TICKSLOCK.acquire();
    let xticks: u32 = TICKS;
    TICKSLOCK.release();
    xticks as usize
}
