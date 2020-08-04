use crate::console::consputc;
use crate::libc;
use crate::spinlock::{release, Spinlock};
use core::ptr;

pub type __builtin_va_list = [__va_list_tag; 1];

#[derive(Copy, Clone)]
pub struct __va_list_tag {
    pub gp_offset: u32,
    pub fp_offset: u32,
    pub overflow_arg_area: *mut libc::c_void,
    pub reg_save_area: *mut libc::c_void,
}

pub type va_list = __builtin_va_list;

#[derive(Copy, Clone)]
pub struct PrintfLock {
    pub lock: Spinlock,
    pub locking: i32,
}

/// formatted console output -- printf, panic.
pub static mut panicked: i32 = 0;
static mut pr: PrintfLock = PrintfLock {
    lock: Spinlock::zeroed(),
    locking: 0,
};
static mut digits: [libc::c_char; 17] = [
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 0,
];
unsafe fn printint(mut xx: i32, mut base: i32, mut sign: i32) {
    let mut buf: [libc::c_char; 16] = [0; 16];
    let mut i: i32 = 0;
    let mut x: u32 = 0;
    if sign != 0 && {
        sign = (xx < 0 as i32) as i32;
        (sign) != 0
    } {
        x = -xx as u32
    } else {
        x = xx as u32
    }
    i = 0 as i32;
    loop {
        let fresh0 = i;
        i += 1;
        buf[fresh0 as usize] = digits[x.wrapping_rem(base as u32) as usize];
        x = (x as u32).wrapping_div(base as u32) as u32 as u32;
        if x == 0 as i32 as u32 {
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
        if i < 0 as i32 {
            break;
        }
        consputc(buf[i as usize] as i32);
    }
}

unsafe fn printptr(mut x: u64) {
    let mut i: i32 = 0;
    consputc('0' as i32);
    consputc('x' as i32);
    i = 0 as i32;
    while (i as u64) < (::core::mem::size_of::<u64>() as u64).wrapping_mul(2 as i32 as u64) {
        consputc(
            digits[(x
                >> (::core::mem::size_of::<u64>() as u64)
                    .wrapping_mul(8 as i32 as u64)
                    .wrapping_sub(4 as i32 as u64)) as usize] as i32,
        );
        i += 1;
        x <<= 4 as i32
    }
}

/// Print to the console. only understands %d, %x, %p, %s.
pub unsafe extern "C" fn printf(mut fmt: *mut libc::c_char, mut args: ...) {
    let mut ap: ::core::ffi::VaListImpl;
    let mut i: i32 = 0;
    let mut c: i32 = 0;
    let mut locking: i32 = 0;
    let mut s: *mut libc::c_char = ptr::null_mut();
    locking = pr.locking;
    if locking != 0 {
        pr.lock.acquire();
    }
    if fmt.is_null() {
        panic(b"null fmt\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ap = args.clone();
    i = 0 as i32;
    loop {
        c = *fmt.offset(i as isize) as i32 & 0xff as i32;
        if c == 0 as i32 {
            break;
        }
        if c != '%' as i32 {
            consputc(c);
        } else {
            i += 1;
            c = *fmt.offset(i as isize) as i32 & 0xff as i32;
            if c == 0 as i32 {
                break;
            }
            match c {
                100 => {
                    printint(ap.as_va_list().arg::<i32>(), 10 as i32, 1 as i32);
                }
                120 => {
                    printint(ap.as_va_list().arg::<i32>(), 16 as i32, 1 as i32);
                }
                112 => {
                    printptr(ap.as_va_list().arg::<u64>());
                }
                115 => {
                    s = ap.as_va_list().arg::<*mut libc::c_char>();
                    if s.is_null() {
                        s = b"(null)\x00" as *const u8 as *const libc::c_char as *mut libc::c_char
                    }
                    while *s != 0 {
                        consputc(*s as i32);
                        s = s.offset(1)
                    }
                }
                37 => {
                    consputc('%' as i32);
                }
                _ => {
                    // Print unknown % sequence to draw attention.
                    consputc('%' as i32);
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

pub unsafe fn panic(mut s: *mut libc::c_char) -> ! {
    pr.locking = 0 as i32;
    printf(b"panic: \x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    printf(s);
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);

    // freeze other CPUs
    ::core::ptr::write_volatile(&mut panicked as *mut i32, 1 as i32);
    loop {}
}

pub unsafe fn printfinit() {
    pr.lock
        .initlock(b"pr\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    pr.locking = 1 as i32;
}
