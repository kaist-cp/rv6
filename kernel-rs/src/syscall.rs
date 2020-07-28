use crate::libc;
use crate::proc::{myproc, proc_0};
use crate::riscv::pagetable_t;
extern "C" {
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn strlen(_: *const libc::c_char) -> i32;
    #[no_mangle]
    fn copyin(_: pagetable_t, _: *mut libc::c_char, _: u64, _: u64) -> i32;
    #[no_mangle]
    fn copyinstr(_: pagetable_t, _: *mut libc::c_char, _: u64, _: u64) -> i32;
    #[no_mangle]
    fn sys_chdir() -> u64;
    #[no_mangle]
    fn sys_close() -> u64;
    #[no_mangle]
    fn sys_dup() -> u64;
    #[no_mangle]
    fn sys_exec() -> u64;
    #[no_mangle]
    fn sys_exit() -> u64;
    #[no_mangle]
    fn sys_fork() -> u64;
    #[no_mangle]
    fn sys_fstat() -> u64;
    #[no_mangle]
    fn sys_getpid() -> u64;
    #[no_mangle]
    fn sys_kill() -> u64;
    #[no_mangle]
    fn sys_link() -> u64;
    #[no_mangle]
    fn sys_mkdir() -> u64;
    #[no_mangle]
    fn sys_mknod() -> u64;
    #[no_mangle]
    fn sys_open() -> u64;
    #[no_mangle]
    fn sys_pipe() -> u64;
    #[no_mangle]
    fn sys_read() -> u64;
    #[no_mangle]
    fn sys_sbrk() -> u64;
    #[no_mangle]
    fn sys_sleep() -> u64;
    #[no_mangle]
    fn sys_unlink() -> u64;
    #[no_mangle]
    fn sys_wait() -> u64;
    #[no_mangle]
    fn sys_write() -> u64;
    #[no_mangle]
    fn sys_uptime() -> u64;
}
/// Fetch the u64 at addr from the current process.
#[no_mangle]
pub unsafe extern "C" fn fetchaddr(mut addr: u64, mut ip: *mut u64) -> i32 {
    let mut p: *mut proc_0 = myproc();
    if addr >= (*p).sz || addr.wrapping_add(::core::mem::size_of::<u64>() as u64) > (*p).sz {
        return -1;
    }
    if copyin(
        (*p).pagetable,
        ip as *mut libc::c_char,
        addr,
        ::core::mem::size_of::<u64>() as u64,
    ) != 0 as i32
    {
        return -1;
    }
    0
}
/// Fetch the nul-terminated string at addr from the current process.
/// Returns length of string, not including nul, or -1 for error.
#[no_mangle]
pub unsafe extern "C" fn fetchstr(mut addr: u64, mut buf: *mut libc::c_char, mut max: i32) -> i32 {
    let mut p: *mut proc_0 = myproc();
    let mut err: i32 = copyinstr((*p).pagetable, buf, addr, max as u64);
    if err < 0 as i32 {
        return err;
    }
    strlen(buf)
}
unsafe extern "C" fn argraw(mut n: i32) -> u64 {
    let mut p: *mut proc_0 = myproc();
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
/// Fetch the nth 32-bit system call argument.
#[no_mangle]
pub unsafe extern "C" fn argint(mut n: i32, mut ip: *mut i32) -> i32 {
    *ip = argraw(n) as i32;
    0
}
/// Retrieve an argument as a pointer.
/// Doesn't check for legality, since
/// copyin/copyout will do that.
#[no_mangle]
pub unsafe extern "C" fn argaddr(mut n: i32, mut ip: *mut u64) -> i32 {
    *ip = argraw(n);
    0
}
/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns string length if OK (including nul), -1 if error.
#[no_mangle]
pub unsafe extern "C" fn argstr(mut n: i32, mut buf: *mut libc::c_char, mut max: i32) -> i32 {
    let mut addr: u64 = 0;
    if argaddr(n, &mut addr) < 0 as i32 {
        return -1;
    }
    fetchstr(addr, buf, max)
}
static mut syscalls: [Option<unsafe extern "C" fn() -> u64>; 22] = unsafe {
    [
        None,
        Some(sys_fork as unsafe extern "C" fn() -> u64),
        Some(sys_exit as unsafe extern "C" fn() -> u64),
        Some(sys_wait as unsafe extern "C" fn() -> u64),
        Some(sys_pipe as unsafe extern "C" fn() -> u64),
        Some(sys_read as unsafe extern "C" fn() -> u64),
        Some(sys_kill as unsafe extern "C" fn() -> u64),
        Some(sys_exec as unsafe extern "C" fn() -> u64),
        Some(sys_fstat as unsafe extern "C" fn() -> u64),
        Some(sys_chdir as unsafe extern "C" fn() -> u64),
        Some(sys_dup as unsafe extern "C" fn() -> u64),
        Some(sys_getpid as unsafe extern "C" fn() -> u64),
        Some(sys_sbrk as unsafe extern "C" fn() -> u64),
        Some(sys_sleep as unsafe extern "C" fn() -> u64),
        Some(sys_uptime as unsafe extern "C" fn() -> u64),
        Some(sys_open as unsafe extern "C" fn() -> u64),
        Some(sys_write as unsafe extern "C" fn() -> u64),
        Some(sys_mknod as unsafe extern "C" fn() -> u64),
        Some(sys_unlink as unsafe extern "C" fn() -> u64),
        Some(sys_link as unsafe extern "C" fn() -> u64),
        Some(sys_mkdir as unsafe extern "C" fn() -> u64),
        Some(sys_close as unsafe extern "C" fn() -> u64),
    ]
};
#[no_mangle]
pub unsafe extern "C" fn syscall() {
    let mut num: i32 = 0;
    let mut p: *mut proc_0 = myproc();
    num = (*(*p).tf).a7 as i32;
    if num > 0
        && (num as u64)
            < (::core::mem::size_of::<[Option<unsafe extern "C" fn() -> u64>; 22]>() as u64)
                .wrapping_div(
                    ::core::mem::size_of::<Option<unsafe extern "C" fn() -> u64>>() as u64,
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
        (*(*p).tf).a0 = -(1 as i32) as u64
    };
}
