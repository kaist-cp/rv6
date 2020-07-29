use crate::{
    libc,
    proc::{exit, fork, growproc, kill, myproc, sleep, wait},
    spinlock::{acquire, release},
    syscall::{argaddr, argint},
    trap::{ticks, tickslock},
};
#[no_mangle]
pub unsafe extern "C" fn sys_exit() -> u64 {
    let mut n: i32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    exit(n);
    0 as i32 as u64
    // not reached
}
#[no_mangle]
pub unsafe extern "C" fn sys_getpid() -> u64 {
    (*myproc()).pid as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_fork() -> u64 {
    fork() as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_wait() -> u64 {
    let mut p: u64 = 0;
    if argaddr(0 as i32, &mut p) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    wait(p) as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_sbrk() -> u64 {
    let mut addr: i32 = 0;
    let mut n: i32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    addr = (*myproc()).sz as i32;
    if growproc(n) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    addr as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_sleep() -> u64 {
    let mut n: i32 = 0;
    let mut ticks0: u32 = 0;
    if argint(0 as i32, &mut n) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    acquire(&mut tickslock);
    ticks0 = ticks;
    while ticks.wrapping_sub(ticks0) < n as u32 {
        if (*myproc()).killed != 0 {
            release(&mut tickslock);
            return -(1 as i32) as u64;
        }
        sleep(&mut ticks as *mut u32 as *mut libc::c_void, &mut tickslock);
    }
    release(&mut tickslock);
    0 as i32 as u64
}
#[no_mangle]
pub unsafe extern "C" fn sys_kill() -> u64 {
    let mut pid: i32 = 0;
    if argint(0 as i32, &mut pid) < 0 as i32 {
        return -(1 as i32) as u64;
    }
    kill(pid) as u64
}
/// return how many clock tick interrupts have occurred
/// since start.
#[no_mangle]
pub unsafe extern "C" fn sys_uptime() -> u64 {
    let mut xticks: u32 = 0;
    acquire(&mut tickslock);
    xticks = ticks;
    release(&mut tickslock);
    xticks as u64
}
