use crate::libc;
use core::ptr;
extern "C" {
    pub type pipe;
    #[no_mangle]
    fn consputc(_: libc::c_int);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
}
pub type __builtin_va_list = [__va_list_tag; 1];
#[derive(Copy, Clone)]
#[repr(C)]
pub struct __va_list_tag {
    pub gp_offset: libc::c_uint,
    pub fp_offset: libc::c_uint,
    pub overflow_arg_area: *mut libc::c_void,
    pub reg_save_area: *mut libc::c_void,
}
pub type va_list = __builtin_va_list;
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// Per-CPU state.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct cpu {
    pub proc_0: *mut proc_0,
    pub scheduler: context,
    pub noff: libc::c_int,
    pub intena: libc::c_int,
}
// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct context {
    pub ra: uint64,
    pub sp: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
}
// Per-process state
#[derive(Copy, Clone)]
#[repr(C)]
pub struct proc_0 {
    pub lock: spinlock,
    pub state: procstate,
    pub parent: *mut proc_0,
    pub chan: *mut libc::c_void,
    pub killed: libc::c_int,
    pub xstate: libc::c_int,
    pub pid: libc::c_int,
    pub kstack: uint64,
    pub sz: uint64,
    pub pagetable: pagetable_t,
    pub tf: *mut trapframe,
    pub context: context,
    pub ofile: [*mut file; 16],
    pub cwd: *mut inode,
    pub name: [libc::c_char; 16],
}
// FD_DEVICE
// in-memory copy of an inode
#[derive(Copy, Clone)]
#[repr(C)]
pub struct inode {
    pub dev: uint,
    pub inum: uint,
    pub ref_0: libc::c_int,
    pub lock: sleeplock,
    pub valid: libc::c_int,
    pub type_0: libc::c_short,
    pub major: libc::c_short,
    pub minor: libc::c_short,
    pub nlink: libc::c_short,
    pub size: uint,
    pub addrs: [uint; 13],
}
// Long-term locks for processes
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sleeplock {
    pub locked: uint,
    pub lk: spinlock,
    pub name: *mut libc::c_char,
    pub pid: libc::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct file {
    pub type_0: C2RustUnnamed,
    pub ref_0: libc::c_int,
    pub readable: libc::c_char,
    pub writable: libc::c_char,
    pub pipe: *mut pipe,
    pub ip: *mut inode,
    pub off: uint,
    pub major: libc::c_short,
}
pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;
// per-process data for the trap handling code in trampoline.S.
// sits in a page by itself just under the trampoline page in the
// user page table. not specially mapped in the kernel page table.
// the sscratch register points here.
// uservec in trampoline.S saves user registers in the trapframe,
// then initializes registers from the trapframe's
// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// usertrapret() and userret in trampoline.S set up
// the trapframe's kernel_*, restore user registers from the
// trapframe, switch to the user page table, and enter user space.
// the trapframe includes callee-saved user registers like s0-s11 because the
// return-to-user path via usertrapret() doesn't return through
// the entire kernel call stack.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct trapframe {
    pub kernel_satp: uint64,
    pub kernel_sp: uint64,
    pub kernel_trap: uint64,
    pub epc: uint64,
    pub kernel_hartid: uint64,
    pub ra: uint64,
    pub sp: uint64,
    pub gp: uint64,
    pub tp: uint64,
    pub t0: uint64,
    pub t1: uint64,
    pub t2: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub a0: uint64,
    pub a1: uint64,
    pub a2: uint64,
    pub a3: uint64,
    pub a4: uint64,
    pub a5: uint64,
    pub a6: uint64,
    pub a7: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
    pub t3: uint64,
    pub t4: uint64,
    pub t5: uint64,
    pub t6: uint64,
}
pub type pagetable_t = *mut uint64;
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
// lock to avoid interleaving concurrent printf's.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub lock: spinlock,
    pub locking: libc::c_int,
}
//
// formatted console output -- printf, panic.
//
#[no_mangle]
pub static mut panicked: libc::c_int = 0 as libc::c_int;
static mut pr: C2RustUnnamed_0 = C2RustUnnamed_0 {
    lock: spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    locking: 0,
};
static mut digits: [libc::c_char; 17] = [
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 0,
];
unsafe extern "C" fn printint(mut xx: libc::c_int, mut base: libc::c_int, mut sign: libc::c_int) {
    let mut buf: [libc::c_char; 16] = [0; 16];
    let mut i: libc::c_int = 0;
    let mut x: uint = 0;
    if sign != 0 && {
        sign = (xx < 0 as libc::c_int) as libc::c_int;
        (sign) != 0
    } {
        x = -xx as uint
    } else {
        x = xx as uint
    }
    i = 0 as libc::c_int;
    loop {
        let fresh0 = i;
        i += 1;
        buf[fresh0 as usize] = digits[x.wrapping_rem(base as libc::c_uint) as usize];
        x = (x as libc::c_uint).wrapping_div(base as libc::c_uint) as uint as uint;
        if x == 0 as libc::c_int as libc::c_uint {
            break;
        }
    }
    if sign != 0 {
        let fresh1 = i;
        i += 1;
        buf[fresh1 as usize] = '-' as i32 as libc::c_char
    }
    loop {
        i -= 1;
        if i < 0 as libc::c_int {
            break;
        }
        consputc(buf[i as usize] as libc::c_int);
    }
}
unsafe extern "C" fn printptr(mut x: uint64) {
    let mut i: libc::c_int = 0;
    consputc('0' as i32);
    consputc('x' as i32);
    i = 0 as libc::c_int;
    while (i as libc::c_ulong)
        < (::core::mem::size_of::<uint64>() as libc::c_ulong)
            .wrapping_mul(2 as libc::c_int as libc::c_ulong)
    {
        consputc(
            digits[(x
                >> (::core::mem::size_of::<uint64>() as libc::c_ulong)
                    .wrapping_mul(8 as libc::c_int as libc::c_ulong)
                    .wrapping_sub(4 as libc::c_int as libc::c_ulong)) as usize]
                as libc::c_int,
        );
        i += 1;
        x <<= 4 as libc::c_int
    }
}
// printf.c
// Print to the console. only understands %d, %x, %p, %s.
#[no_mangle]
pub unsafe extern "C" fn printf(mut fmt: *mut libc::c_char, mut args: ...) {
    let mut ap: ::core::ffi::VaListImpl;
    let mut i: libc::c_int = 0;
    let mut c: libc::c_int = 0;
    let mut locking: libc::c_int = 0;
    let mut s: *mut libc::c_char = ptr::null_mut();
    locking = pr.locking;
    if locking != 0 {
        acquire(&mut pr.lock);
    }
    if fmt.is_null() {
        panic(b"null fmt\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ap = args.clone();
    i = 0 as libc::c_int;
    loop {
        c = *fmt.offset(i as isize) as libc::c_int & 0xff as libc::c_int;
        if c == 0 as libc::c_int {
            break;
        }
        if c != '%' as i32 {
            consputc(c);
        } else {
            i += 1;
            c = *fmt.offset(i as isize) as libc::c_int & 0xff as libc::c_int;
            if c == 0 as libc::c_int {
                break;
            }
            match c {
                100 => {
                    printint(
                        ap.as_va_list().arg::<libc::c_int>(),
                        10 as libc::c_int,
                        1 as libc::c_int,
                    );
                }
                120 => {
                    printint(
                        ap.as_va_list().arg::<libc::c_int>(),
                        16 as libc::c_int,
                        1 as libc::c_int,
                    );
                }
                112 => {
                    printptr(ap.as_va_list().arg::<uint64>());
                }
                115 => {
                    s = ap.as_va_list().arg::<*mut libc::c_char>();
                    if s.is_null() {
                        s = b"(null)\x00" as *const u8 as *const libc::c_char as *mut libc::c_char
                    }
                    while *s != 0 {
                        consputc(*s as libc::c_int);
                        s = s.offset(1)
                    }
                }
                37 => {
                    consputc('%' as i32);
                }
                _ => {
                    // Print unknown % sequence to draw attention.
                    consputc('%' as i32); // freeze other CPUs
                    consputc(c);
                }
            }
        }
        i += 1
    }
    if locking != 0 {
        release(&mut pr.lock);
    };
}
#[no_mangle]
pub unsafe extern "C" fn panic(mut s: *mut libc::c_char) -> ! {
    pr.locking = 0 as libc::c_int;
    printf(b"panic: \x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    printf(s);
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    ::core::ptr::write_volatile(&mut panicked as *mut libc::c_int, 1 as libc::c_int);
    loop {}
}
#[no_mangle]
pub unsafe extern "C" fn printfinit() {
    initlock(
        &mut pr.lock,
        b"pr\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    pr.locking = 1 as libc::c_int;
}
