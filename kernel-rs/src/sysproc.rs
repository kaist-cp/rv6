use crate::{
    libc,
    proc::{exit, fork, growproc, kill, myproc, sleep, wait},
    syscall::{argaddr, argint},
    trap::{ticks, tickslock},
};

pub unsafe fn sys_exit() -> usize {
    let mut n: i32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return usize::MAX;
    }
    exit(n);
    0 as i32 as usize
    // not reached
}

pub unsafe fn sys_getpid() -> usize {
    (*myproc()).pid as usize
}

pub unsafe fn sys_fork() -> usize {
    fork() as usize
}

pub unsafe fn sys_wait() -> usize {
    let mut p: usize = 0;
    if argaddr(0 as i32, &mut p) < 0 as i32 {
        return usize::MAX;
    }
    wait(p) as usize
}

pub unsafe fn sys_sbrk() -> usize {
    let mut addr: i32 = 0;
    let mut n: i32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return usize::MAX;
    }
    addr = (*myproc()).sz as i32;
    if growproc(n) < 0 as i32 {
        return usize::MAX;
    }
    addr as usize
}

pub unsafe fn sys_sleep() -> usize {
    let mut n: i32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return usize::MAX;
    }
    tickslock.acquire();
    let ticks0 = ticks;
    while ticks.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed != 0 {
            tickslock.release();
            return usize::MAX;
        }
        sleep(&mut ticks as *mut u32 as *mut libc::c_void, &mut tickslock);
    }
    tickslock.release();
    0 as i32 as usize
}

pub unsafe fn sys_kill() -> usize {
    let mut pid: i32 = 0;
    if argint(0 as i32, &mut pid) < 0 as i32 {
        return usize::MAX;
    }
    kill(pid) as usize
}

/// return how many clock tick interrupts have occurred
/// since start.
pub unsafe fn sys_uptime() -> usize {
    let mut xticks: u32 = 0;
    tickslock.acquire();
    xticks = ticks;
    tickslock.release();
    xticks as usize
}
