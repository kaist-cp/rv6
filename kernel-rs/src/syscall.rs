use crate::{
    println,
    proc::{myproc, Proc},
    string::strlen,
    sysfile::*,
    sysproc::*,
    vm::{copyin, copyinstr},
};
use core::str;

/// Fetch the usize at addr from the current process.
pub unsafe fn fetchaddr(addr: usize, ip: *mut usize) -> i32 {
    let p: *mut Proc = myproc();
    if addr >= (*p).sz || addr.wrapping_add(::core::mem::size_of::<usize>()) > (*p).sz {
        return -1;
    }
    if copyin(
        &mut (*p).pagetable,
        ip as *mut u8,
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
pub unsafe fn fetchstr(addr: usize, buf: *mut u8, max: usize) -> i32 {
    let p: *mut Proc = myproc();
    let err: i32 = copyinstr(&mut (*p).pagetable, buf, addr, max);
    if err < 0 {
        return err;
    }
    strlen(buf)
}

unsafe fn argraw(n: usize) -> usize {
    let p = myproc();
    match n {
        0 => (*(*p).tf).a0,
        1 => (*(*p).tf).a1,
        2 => (*(*p).tf).a2,
        3 => (*(*p).tf).a3,
        4 => (*(*p).tf).a4,
        5 => (*(*p).tf).a5,
        _ => panic!("argraw"),
    }
}

/// Fetch the nth 32-bit system call argument.
pub unsafe fn argint(n: usize) -> Result<i32, ()> {
    Ok(argraw(n) as i32)
}

/// Retrieve an argument as a pointer.
/// Doesn't check for legality, since
/// copyin/copyout will do that.
pub unsafe fn argaddr(n: usize) -> Result<usize, ()> {
    Ok(argraw(n))
}

/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns string length if OK (including nul), -1 if error.
pub unsafe fn argstr(n: usize, buf: *mut u8, max: usize) -> Result<i32, ()> {
    let addr = argaddr(n)?;
    Ok(fetchstr(addr, buf, max))
}

static mut SYSCALLS: [Option<unsafe fn() -> usize>; 22] = [
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
];

pub unsafe fn syscall() {
    let mut p: *mut Proc = myproc();
    let num: i32 = (*(*p).tf).a7 as i32;
    if num > 0 && (num as usize) < SYSCALLS.len() && SYSCALLS[num as usize].is_some() {
        (*(*p).tf).a0 = SYSCALLS[num as usize].expect("non-null function pointer")()
    } else {
        println!(
            "{} {}: unknown sys call {}",
            (*p).pid,
            str::from_utf8(&(*p).name).unwrap_or("???"),
            num
        );
        (*(*p).tf).a0 = usize::MAX
    };
}
