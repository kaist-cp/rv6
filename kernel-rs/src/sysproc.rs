use crate::{
    libc,
    printf::panic,
    proc::{exit, fork, growproc, kill, myproc, sleep, wait},
    syscall::{argaddr, argint},
    trap::{TICKS, TICKSLOCK},
};

pub unsafe fn sys_exit() -> usize {
    let mut n: i32 = 0;
    if argint(0, &mut n) < 0 {
        return usize::MAX;
    }
    exit(n);

    panic(b"sys_exit: not reached\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
}

pub unsafe fn sys_getpid() -> usize {
    (*myproc()).pid as _
}

pub unsafe fn sys_fork() -> usize {
    fork() as _
}

pub unsafe fn sys_wait() -> usize {
    let mut p: usize = 0;
    if argaddr(0, &mut p) < 0 {
        return usize::MAX;
    }
    wait(p) as _
}

pub unsafe fn sys_sbrk() -> usize {
    let mut n: i32 = 0;
    if argint(0, &mut n) < 0 {
        return usize::MAX;
    }
    let addr: i32 = (*myproc()).sz as i32;
    if growproc(n) < 0 {
        return usize::MAX;
    }
    addr as usize
}

pub unsafe fn sys_sleep() -> usize {
    let mut n: i32 = 0;
    if argint(0, &mut n) < 0 {
        return usize::MAX;
    }
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
    let mut pid: i32 = 0;
    if argint(0, &mut pid) < 0 {
        return usize::MAX;
    }
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
