use crate::libc;
use crate::{
    printf::{panic, printf},
    proc::{myproc, proc},
    string::strlen,
    sysfile::*,
    sysproc::*,
    vm::{copyin, copyinstr},
};

/// Fetch the usize at addr from the current process.
pub unsafe fn fetchaddr(mut addr: usize, mut ip: *mut usize) -> i32 {
    let mut p: *mut proc = myproc();
    if addr >= (*p).sz || addr.wrapping_add(::core::mem::size_of::<usize>()) > (*p).sz {
        return -1;
    }
    if copyin(
        (*p).pagetable,
        ip as *mut libc::c_char,
        addr,
        ::core::mem::size_of::<usize>(),
    ) != 0
    {
        return -1;
    }
    0
}

/// Fetch the nul-terminated string at addr from the current process.
/// Returns length of string, not including nul, or -1 for error.
pub unsafe fn fetchstr(mut addr: usize, mut buf: *mut libc::c_char, mut max: i32) -> i32 {
    let mut p: *mut proc = myproc();
    let mut err: i32 = copyinstr((*p).pagetable, buf, addr, max as usize);
    if err < 0 {
        return err;
    }
    strlen(buf)
}

unsafe fn argraw(mut n: i32) -> usize {
    let mut p: *mut proc = myproc();
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

/// Fetch the nth 32-bit system call argument.
pub unsafe fn argint(mut n: i32, mut ip: *mut i32) -> i32 {
    *ip = argraw(n) as i32;
    0
}

/// Retrieve an argument as a pointer.
/// Doesn't check for legality, since
/// copyin/copyout will do that.
pub unsafe fn argaddr(mut n: i32, mut ip: *mut usize) -> i32 {
    *ip = argraw(n);
    0
}

/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns string length if OK (including nul), -1 if error.
pub unsafe fn argstr(mut n: i32, mut buf: *mut libc::c_char, mut max: i32) -> i32 {
    let mut addr: usize = 0;
    if argaddr(n, &mut addr) < 0 {
        return -1;
    }
    fetchstr(addr, buf, max)
}

static mut syscalls: [Option<unsafe fn() -> usize>; 22] = unsafe {
    [
        None,
        Some(sys_fork as unsafe fn() -> usize),
        Some(sys_exit as unsafe fn() -> usize),
        Some(sys_wait as unsafe fn() -> usize),
        Some(sys_pipe as unsafe fn() -> usize),
        Some(sys_read as unsafe fn() -> usize),
        Some(sys_kill as unsafe fn() -> usize),
        Some(sys_exec as unsafe fn() -> usize),
        Some(sys_fstat as unsafe fn() -> usize),
        Some(sys_chdir as unsafe fn() -> usize),
        Some(sys_dup as unsafe fn() -> usize),
        Some(sys_getpid as unsafe fn() -> usize),
        Some(sys_sbrk as unsafe fn() -> usize),
        Some(sys_sleep as unsafe fn() -> usize),
        Some(sys_uptime as unsafe fn() -> usize),
        Some(sys_open as unsafe fn() -> usize),
        Some(sys_write as unsafe fn() -> usize),
        Some(sys_mknod as unsafe fn() -> usize),
        Some(sys_unlink as unsafe fn() -> usize),
        Some(sys_link as unsafe fn() -> usize),
        Some(sys_mkdir as unsafe fn() -> usize),
        Some(sys_close as unsafe fn() -> usize),
    ]
};

pub unsafe fn syscall() {
    let mut num: i32 = 0;
    let mut p: *mut proc = myproc();
    num = (*(*p).tf).a7 as i32;
    if num > 0
        && (num as usize)
            < (::core::mem::size_of::<[Option<unsafe fn() -> usize>; 22]>())
                .wrapping_div(::core::mem::size_of::<Option<unsafe fn() -> usize>>())
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
        (*(*p).tf).a0 = usize::MAX
    };
}
