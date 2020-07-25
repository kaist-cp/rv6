use crate::libc;
use crate::proc::cpu;
use crate::spinlock::{acquire, initlock, release, Spinlock};
use core::ptr;
extern "C" {
    pub type pipe;
    #[no_mangle]
    fn consputc(_: libc::c_int);
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

#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub lock: Spinlock,
    pub locking: libc::c_int,
}
//
// formatted console output -- printf, panic.
//
#[no_mangle]
pub static mut panicked: libc::c_int = 0 as libc::c_int;
static mut pr: C2RustUnnamed_0 = C2RustUnnamed_0 {
    lock: Spinlock {
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
