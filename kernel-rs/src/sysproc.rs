use crate::{ libc, proc::proc_0, spinlock::Spinlock};
extern "C" {
    // pub type file;
    // pub type inode;
    #[no_mangle]
    fn exit(_: libc::c_int);
    #[no_mangle]
    fn fork() -> libc::c_int;
    #[no_mangle]
    fn growproc(_: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn kill(_: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut Spinlock);
    #[no_mangle]
    fn wait(_: uint64) -> libc::c_int;
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut Spinlock);
    #[no_mangle]
    fn release(_: *mut Spinlock);
    // syscall.c
    #[no_mangle]
    fn argint(_: libc::c_int, _: *mut libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn argaddr(_: libc::c_int, _: *mut uint64) -> libc::c_int;
    // trap.c
    #[no_mangle]
    static mut ticks: uint;
    #[no_mangle]
    static mut tickslock: Spinlock;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;
// // Saved registers for kernel context switches.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct context {
//     pub ra: uint64,
//     pub sp: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
// }
// // Per-process state
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct proc_0 {
//     pub lock: spinlock,
//     pub state: procstate,
//     pub parent: *mut proc_0,
//     pub chan: *mut libc::c_void,
//     pub killed: libc::c_int,
//     pub xstate: libc::c_int,
//     pub pid: libc::c_int,
//     pub kstack: uint64,
//     pub sz: uint64,
//     pub pagetable: pagetable_t,
//     pub tf: *mut trapframe,
//     pub context: context,
//     pub ofile: [*mut file; 16],
//     pub cwd: *mut inode,
//     pub name: [libc::c_char; 16],
// }
// // per-process data for the trap handling code in trampoline.S.
// // sits in a page by itself just under the trampoline page in the
// // user page table. not specially mapped in the kernel page table.
// // the sscratch register points here.
// // uservec in trampoline.S saves user registers in the trapframe,
// // then initializes registers from the trapframe's
// // kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// // usertrapret() and userret in trampoline.S set up
// // the trapframe's kernel_*, restore user registers from the
// // trapframe, switch to the user page table, and enter user space.
// // the trapframe includes callee-saved user registers like s0-s11 because the
// // return-to-user path via usertrapret() doesn't return through
// // the entire kernel call stack.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct trapframe {
//     pub kernel_satp: uint64,
//     pub kernel_sp: uint64,
//     pub kernel_trap: uint64,
//     pub epc: uint64,
//     pub kernel_hartid: uint64,
//     pub ra: uint64,
//     pub sp: uint64,
//     pub gp: uint64,
//     pub tp: uint64,
//     pub t0: uint64,
//     pub t1: uint64,
//     pub t2: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub a0: uint64,
//     pub a1: uint64,
//     pub a2: uint64,
//     pub a3: uint64,
//     pub a4: uint64,
//     pub a5: uint64,
//     pub a6: uint64,
//     pub a7: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
//     pub t3: uint64,
//     pub t4: uint64,
//     pub t5: uint64,
//     pub t6: uint64,
// }
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
// // Mutual exclusion lock.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct spinlock {
//     pub locked: uint,
//     pub name: *mut libc::c_char,
//     pub cpu: *mut cpu,
// }
// // Per-CPU state.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct cpu {
//     pub proc_0: *mut proc_0,
//     pub scheduler: context,
//     pub noff: libc::c_int,
//     pub intena: libc::c_int,
// }
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
// return how many clock tick interrupts have occurred
// since start.
#[no_mangle]
pub unsafe extern "C" fn sys_uptime() -> uint64 {
    let mut xticks: uint = 0;
    acquire(&mut tickslock);
    xticks = ticks;
    release(&mut tickslock);
    xticks as uint64
}
