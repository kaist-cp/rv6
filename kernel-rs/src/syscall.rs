use crate::{ libc, proc };
extern "C" {
    pub type inode;
    pub type file;
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn strlen(_: *const libc::c_char) -> libc::c_int;
    #[no_mangle]
    fn copyin(_: pagetable_t, _: *mut libc::c_char, _: uint64, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn copyinstr(_: pagetable_t, _: *mut libc::c_char, _: uint64, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn sys_chdir() -> uint64;
    #[no_mangle]
    fn sys_close() -> uint64;
    #[no_mangle]
    fn sys_dup() -> uint64;
    #[no_mangle]
    fn sys_exec() -> uint64;
    #[no_mangle]
    fn sys_exit() -> uint64;
    #[no_mangle]
    fn sys_fork() -> uint64;
    #[no_mangle]
    fn sys_fstat() -> uint64;
    #[no_mangle]
    fn sys_getpid() -> uint64;
    #[no_mangle]
    fn sys_kill() -> uint64;
    #[no_mangle]
    fn sys_link() -> uint64;
    #[no_mangle]
    fn sys_mkdir() -> uint64;
    #[no_mangle]
    fn sys_mknod() -> uint64;
    #[no_mangle]
    fn sys_open() -> uint64;
    #[no_mangle]
    fn sys_pipe() -> uint64;
    #[no_mangle]
    fn sys_read() -> uint64;
    #[no_mangle]
    fn sys_sbrk() -> uint64;
    #[no_mangle]
    fn sys_sleep() -> uint64;
    #[no_mangle]
    fn sys_unlink() -> uint64;
    #[no_mangle]
    fn sys_wait() -> uint64;
    #[no_mangle]
    fn sys_write() -> uint64;
    #[no_mangle]
    fn sys_uptime() -> uint64;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;

pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
// Fetch the uint64 at addr from the current process.
#[no_mangle]
pub unsafe extern "C" fn fetchaddr(mut addr: uint64, mut ip: *mut uint64) -> libc::c_int {
    let mut p: *mut proc::proc_0 = proc::myproc();
    if addr >= (*p).sz
        || addr.wrapping_add(::core::mem::size_of::<uint64>() as libc::c_ulong) > (*p).sz
    {
        return -(1 as libc::c_int);
    }
    if copyin(
        (*p).pagetable,
        ip as *mut libc::c_char,
        addr,
        ::core::mem::size_of::<uint64>() as libc::c_ulong,
    ) != 0 as libc::c_int
    {
        return -(1 as libc::c_int);
    }
    0 as libc::c_int
}
// Fetch the nul-terminated string at addr from the current process.
// Returns length of string, not including nul, or -1 for error.
#[no_mangle]
pub unsafe extern "C" fn fetchstr(
    mut addr: uint64,
    mut buf: *mut libc::c_char,
    mut max: libc::c_int,
) -> libc::c_int {
    let mut p: *mut proc::proc_0 = proc::myproc();
    let mut err: libc::c_int = copyinstr((*p).pagetable, buf, addr, max as uint64);
    if err < 0 as libc::c_int {
        return err;
    }
    strlen(buf)
}
unsafe extern "C" fn argraw(mut n: libc::c_int) -> uint64 {
    let mut p: *mut proc::proc_0 = proc::myproc();
    match n {
        0 => return (*(*p).tf).a0,
        1 => return (*(*p).tf).a1,
        2 => return (*(*p).tf).a2,
        3 => return (*(*p).tf).a3,
        4 => return (*(*p).tf).a4,
        5 => return (*(*p).tf).a5,
        _ => {}
    }
    panic(b"argraw\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
// syscall.c
// Fetch the nth 32-bit system call argument.
#[no_mangle]
pub unsafe extern "C" fn argint(mut n: libc::c_int, mut ip: *mut libc::c_int) -> libc::c_int {
    *ip = argraw(n) as libc::c_int;
    0 as libc::c_int
}
// Retrieve an argument as a pointer.
// Doesn't check for legality, since
// copyin/copyout will do that.
#[no_mangle]
pub unsafe extern "C" fn argaddr(mut n: libc::c_int, mut ip: *mut uint64) -> libc::c_int {
    *ip = argraw(n);
    0 as libc::c_int
}
// Fetch the nth word-sized system call argument as a null-terminated string.
// Copies into buf, at most max.
// Returns string length if OK (including nul), -1 if error.
#[no_mangle]
pub unsafe extern "C" fn argstr(
    mut n: libc::c_int,
    mut buf: *mut libc::c_char,
    mut max: libc::c_int,
) -> libc::c_int {
    let mut addr: uint64 = 0;
    if argaddr(n, &mut addr) < 0 as libc::c_int {
        return -(1 as libc::c_int);
    }
    fetchstr(addr, buf, max)
}
static mut syscalls: [Option<unsafe extern "C" fn() -> uint64>; 22] = unsafe {
    [
        None,
        Some(sys_fork as unsafe extern "C" fn() -> uint64),
        Some(sys_exit as unsafe extern "C" fn() -> uint64),
        Some(sys_wait as unsafe extern "C" fn() -> uint64),
        Some(sys_pipe as unsafe extern "C" fn() -> uint64),
        Some(sys_read as unsafe extern "C" fn() -> uint64),
        Some(sys_kill as unsafe extern "C" fn() -> uint64),
        Some(sys_exec as unsafe extern "C" fn() -> uint64),
        Some(sys_fstat as unsafe extern "C" fn() -> uint64),
        Some(sys_chdir as unsafe extern "C" fn() -> uint64),
        Some(sys_dup as unsafe extern "C" fn() -> uint64),
        Some(sys_getpid as unsafe extern "C" fn() -> uint64),
        Some(sys_sbrk as unsafe extern "C" fn() -> uint64),
        Some(sys_sleep as unsafe extern "C" fn() -> uint64),
        Some(sys_uptime as unsafe extern "C" fn() -> uint64),
        Some(sys_open as unsafe extern "C" fn() -> uint64),
        Some(sys_write as unsafe extern "C" fn() -> uint64),
        Some(sys_mknod as unsafe extern "C" fn() -> uint64),
        Some(sys_unlink as unsafe extern "C" fn() -> uint64),
        Some(sys_link as unsafe extern "C" fn() -> uint64),
        Some(sys_mkdir as unsafe extern "C" fn() -> uint64),
        Some(sys_close as unsafe extern "C" fn() -> uint64),
    ]
};
#[no_mangle]
pub unsafe extern "C" fn syscall() {
    let mut num: libc::c_int = 0;
    let mut p: *mut proc::proc_0 = proc::myproc();
    num = (*(*p).tf).a7 as libc::c_int;
    if num > 0 as libc::c_int
        && (num as libc::c_ulong)
            < (::core::mem::size_of::<[Option<unsafe extern "C" fn() -> uint64>; 22]>()
                as libc::c_ulong)
                .wrapping_div(
                    ::core::mem::size_of::<Option<unsafe extern "C" fn() -> uint64>>()
                        as libc::c_ulong,
                )
        && syscalls[num as usize].is_some()
    {
        (*(*p).tf).a0 = syscalls[num as usize].expect("non-null function pointer")()
    } else {
        printf(
            b"%d %s: unknown sys call %d\n\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
            (*p).pid,
            (*p).name.as_mut_ptr(),
            num,
        );
        (*(*p).tf).a0 = -(1 as libc::c_int) as uint64
    };
}
