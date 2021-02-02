use crate::{
    kernel::Kernel,
    println,
    proc::CurrentProc,
    vm::{UVAddr, VAddr},
};
use core::{mem, slice, str};
use cstr_core::CStr;

/// Fetch the usize at addr from the current process.
/// Returns Ok(fetched integer) on success, Err(()) on error.
pub unsafe fn fetchaddr(addr: UVAddr, data: &mut CurrentProc) -> Result<usize, ()> {
    let mut ip = 0;
    if addr.into_usize() >= data.memory.size()
        || addr.into_usize().wrapping_add(mem::size_of::<usize>()) > data.memory.size()
    {
        return Err(());
    }
    data.memory.copy_in(
        unsafe {
            slice::from_raw_parts_mut(&mut ip as *mut usize as *mut u8, mem::size_of::<usize>())
        },
        addr,
    )?;
    Ok(ip)
}

/// Fetch the nul-terminated string at addr from the current process.
/// Returns reference to the string in the buffer.
pub unsafe fn fetchstr<'a>(
    addr: UVAddr,
    buf: &mut [u8],
    p: &mut CurrentProc,
) -> Result<&'a CStr, ()> {
    p.memory.copy_in_str(buf, addr)?;

    Ok(unsafe { CStr::from_ptr(buf.as_ptr()) })
}

fn argraw(n: usize, data: &CurrentProc) -> usize {
    match n {
        0 => data.trap_frame().a0,
        1 => data.trap_frame().a1,
        2 => data.trap_frame().a2,
        3 => data.trap_frame().a3,
        4 => data.trap_frame().a4,
        5 => data.trap_frame().a5,
        _ => panic!("argraw"),
    }
}

/// Fetch the nth 32-bit system call argument.
pub fn argint(n: usize, p: &CurrentProc) -> Result<i32, ()> {
    Ok(argraw(n, p) as i32)
}

/// Retrieve an argument as a pointer.
/// Doesn't check for legality, since
/// copyin/copyout will do that.
pub fn argaddr(n: usize, p: &CurrentProc) -> Result<usize, ()> {
    Ok(argraw(n, p))
}

/// Fetch the nth word-sized system call argument as a null-terminated string.
/// Copies into buf, at most max.
/// Returns reference to the string in the buffer.
pub unsafe fn argstr<'a>(n: usize, buf: &mut [u8], p: &mut CurrentProc) -> Result<&'a CStr, ()> {
    let addr = argaddr(n, p)?;
    unsafe { fetchstr(UVAddr::new(addr), buf, p) }
}

impl Kernel {
    pub unsafe fn syscall(&'static self, num: i32, proc: &mut CurrentProc) -> Result<usize, ()> {
        match num {
            1 => unsafe { self.sys_fork(proc) },
            2 => unsafe { self.sys_exit(proc) },
            3 => unsafe { self.sys_wait(proc) },
            4 => self.sys_pipe(proc),
            5 => unsafe { self.sys_read(proc) },
            6 => self.sys_kill(proc),
            7 => unsafe { self.sys_exec(proc) },
            8 => unsafe { self.sys_fstat(proc) },
            9 => unsafe { self.sys_chdir(proc) },
            10 => unsafe { self.sys_dup(proc) },
            11 => unsafe { self.sys_getpid(proc) },
            12 => self.sys_sbrk(proc),
            13 => self.sys_sleep(proc),
            14 => self.sys_uptime(),
            15 => unsafe { self.sys_open(proc) },
            16 => unsafe { self.sys_write(proc) },
            17 => unsafe { self.sys_mknod(proc) },
            18 => unsafe { self.sys_unlink(proc) },
            19 => unsafe { self.sys_link(proc) },
            20 => unsafe { self.sys_mkdir(proc) },
            21 => unsafe { self.sys_close(proc) },
            22 => self.sys_poweroff(proc),
            _ => {
                println!(
                    "{} {}: unknown sys call {}",
                    unsafe { proc.pid() },
                    str::from_utf8(&proc.name).unwrap_or("???"),
                    num
                );
                Err(())
            }
        }
    }
}
