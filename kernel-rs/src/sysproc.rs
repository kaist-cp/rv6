use crate::{
    libc,
    proc::{exit, fork, growproc, kill, myproc, sleep, wait},
    spinlock::{acquire, release},
    syscall::{argaddr, argint},
    trap::{ticks, tickslock},
    utils::spin_loop,
};

pub unsafe fn sys_exit() -> u64 {
    let mut n: i32 = 0;
    if argint(0, &mut n) < 0 {
        return 1 as _;
    }
    exit(n);

    // not reached
    spin_loop()
}

pub unsafe fn sys_getpid() -> u64 {
    (*myproc()).pid as _
}

pub unsafe fn sys_fork() -> u64 {
    fork() as _
}

pub unsafe fn sys_wait() -> u64 {
    let mut p: u64 = 0;
    if argaddr(0, &mut p) < 0 {
        return -1 as _;
    }
    wait(p) as _
}

pub unsafe fn sys_sbrk() -> u64 {
    let mut addr: i32 = 0;
    let mut n: i32 = 0;
    if argint(0, &mut n) < 0 {
        return -1 as _;
    }
    addr = (*myproc()).sz as i32;
    if growproc(n) < 0 {
        return -1 as _;
    }
    addr as u64
}

pub unsafe fn sys_sleep() -> u64 {
    let mut n: i32 = 0;
    let mut ticks0: u32 = 0;
    if argint(0, &mut n) < 0 {
        return -1 as _;
    }
    acquire(&mut tickslock);
    ticks0 = ticks;
    while ticks.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed != 0 {
            release(&mut tickslock);
            return -1 as _;
        }
        sleep(&mut ticks as *mut u32 as *mut libc::c_void, &mut tickslock);
    }
    release(&mut tickslock);
    0
}

pub unsafe fn sys_kill() -> u64 {
    let mut pid: i32 = 0;
    if argint(0, &mut pid) < 0 {
        return -1 as _;
    }
    kill(pid) as u64
}

/// return how many clock tick interrupts have occurred
/// since start.
pub unsafe fn sys_uptime() -> u64 {
    let mut xticks: u32 = 0;
    acquire(&mut tickslock);
    xticks = ticks;
    release(&mut tickslock);
    xticks as u64
}
