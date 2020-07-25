use crate::{
    libc,
    proc::{myproc, sleep},
    spinlock::{acquire, release},
    trap::tickslock,
};
extern "C" {
    #[no_mangle]
    fn exit(_: libc::c_int);
    #[no_mangle]
    fn fork() -> libc::c_int;
    #[no_mangle]
    fn growproc(_: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn kill(_: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn wait(_: uint64) -> libc::c_int;
    // syscall.c
    #[no_mangle]
    fn argint(_: libc::c_int, _: *mut libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn argaddr(_: libc::c_int, _: *mut uint64) -> libc::c_int;
    // trap.c
    #[no_mangle]
    static mut ticks: uint;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;
#[no_mangle]
pub unsafe extern "C" fn sys_exit() -> uint64 {
    let mut n: libc::c_int = 0;
    if argint(0 as libc::c_int, &mut n) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    exit(n);
    0 as libc::c_int as uint64
    // not reached
}
#[no_mangle]
pub unsafe extern "C" fn sys_getpid() -> uint64 {
    (*myproc()).pid as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_fork() -> uint64 {
    fork() as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_wait() -> uint64 {
    let mut p: uint64 = 0;
    if argaddr(0 as libc::c_int, &mut p) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    wait(p) as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_sbrk() -> uint64 {
    let mut addr: libc::c_int = 0;
    let mut n: libc::c_int = 0;
    if argint(0 as libc::c_int, &mut n) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    addr = (*myproc()).sz as libc::c_int;
    if growproc(n) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    addr as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_sleep() -> uint64 {
    let mut n: libc::c_int = 0;
    let mut ticks0: uint = 0;
    if argint(0 as libc::c_int, &mut n) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    acquire(&mut tickslock);
    ticks0 = ticks;
    while ticks.wrapping_sub(ticks0) < n as libc::c_uint {
        if (*myproc()).killed != 0 {
            release(&mut tickslock);
            return -(1 as libc::c_int) as uint64;
        }
        sleep(&mut ticks as *mut uint as *mut libc::c_void, &mut tickslock);
    }
    release(&mut tickslock);
    0 as libc::c_int as uint64
}
#[no_mangle]
pub unsafe extern "C" fn sys_kill() -> uint64 {
    let mut pid: libc::c_int = 0;
    if argint(0 as libc::c_int, &mut pid) < 0 as libc::c_int {
        return -(1 as libc::c_int) as uint64;
    }
    kill(pid) as uint64
}
/// return how many clock tick interrupts have occurred
/// since start.
#[no_mangle]
pub unsafe extern "C" fn sys_uptime() -> uint64 {
    let mut xticks: uint = 0;
    acquire(&mut tickslock);
    xticks = ticks;
    release(&mut tickslock);
    xticks as uint64
}
