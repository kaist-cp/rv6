use core::{mem, str};

use cstr_core::CStr;

use crate::{
    kernel::Kernel,
    println,
    proc::CurrentProc,
    vm::{Addr, UVAddr},
};

/// Fetch the usize at addr from the current process.
/// Returns Ok(fetched integer) on success, Err(()) on error.
pub fn fetchaddr(addr: UVAddr, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
    let mut ip = 0;
    let sz = mem::size_of::<usize>();
    if addr.into_usize() >= proc.memory().size() || addr.into_usize() + sz > proc.memory().size() {
        return Err(());
    }
    proc.memory_mut().copy_in(&mut ip, addr)?;
    Ok(ip)
}

/// Fetch the nul-terminated string at addr from the current process.
/// Returns reference to the string in the buffer.
pub fn fetchstr<'a>(
    addr: UVAddr,
    buf: &'a mut [u8],
    proc: &mut CurrentProc<'_>,
) -> Result<&'a CStr, ()> {
    proc.memory_mut().copy_in_str(buf, addr)?;

    // SAFETY: buf contains '\0' as copy_in_str has succeeded.
    Ok(unsafe { CStr::from_ptr(buf.as_ptr()) })
}

fn argraw(n: usize, proc: &CurrentProc<'_>) -> usize {
    match n {
        0 => proc.trap_frame().a0,
        1 => proc.trap_frame().a1,
        2 => proc.trap_frame().a2,
        3 => proc.trap_frame().a3,
        4 => proc.trap_frame().a4,
        5 => proc.trap_frame().a5,
        _ => panic!("argraw"),
    }
}

/// Fetch the nth 32-bit system call argument.
pub fn argint(n: usize, proc: &CurrentProc<'_>) -> Result<i32, ()> {
    Ok(argraw(n, proc) as i32)
}

/// Retrieve an argument as a pointer.
/// Doesn't check for legality, since
/// copyin/copyout will do that.
pub fn argaddr(n: usize, proc: &CurrentProc<'_>) -> Result<usize, ()> {
    Ok(argraw(n, proc))
}

/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns reference to the string in the buffer.
pub fn argstr<'a>(n: usize, buf: &'a mut [u8], proc: &mut CurrentProc<'_>) -> Result<&'a CStr, ()> {
    let addr = argaddr(n, proc)?;
    fetchstr(addr.into(), buf, proc)
}

impl Kernel {
    pub fn syscall(&'static self, num: i32, proc: &mut CurrentProc<'_>) -> Result<usize, ()> {
        match num {
            1 => self.sys_fork(proc),
            2 => self.sys_exit(proc),
            3 => self.sys_wait(proc),
            4 => self.sys_pipe(proc),
            5 => self.sys_read(proc),
            6 => self.sys_kill(proc),
            7 => self.sys_exec(proc),
            8 => self.sys_fstat(proc),
            9 => self.sys_chdir(proc),
            10 => self.sys_dup(proc),
            11 => self.sys_getpid(proc),
            12 => self.sys_sbrk(proc),
            13 => self.sys_sleep(proc),
            14 => self.sys_uptime(proc),
            15 => self.sys_open(proc),
            16 => self.sys_write(proc),
            17 => self.sys_mknod(proc),
            18 => self.sys_unlink(proc),
            19 => self.sys_link(proc),
            20 => self.sys_mkdir(proc),
            21 => self.sys_close(proc),
            22 => self.sys_poweroff(proc),
            _ => {
                println!(
                    "{} {}: unknown sys call {}",
                    proc.pid(),
                    str::from_utf8(&proc.deref_data().name).unwrap_or("???"),
                    num
                );
                Err(())
            }
        }
    }
}
