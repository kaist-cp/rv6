//! formatted console output -- printf, panic.
use crate::console::consputc;
use crate::libc;
use crate::spinlock::Spinlock;
use core::fmt;

/// lock to avoid interleaving concurrent printf's.
#[derive(Copy, Clone)]
struct PrintfLock {
    lock: Spinlock,
    locking: i32,
}

pub struct Writer {
    column_position: usize,
    // prr: PrintfLock,
}

/// A global Writer instance that can be used for printing to the VGA text buffer.
///
/// Used by the print! and println! macros.
pub static mut WRITER: Writer = Writer {
    column_position: 0,
    // prr: PrintfLock::zeroed(),
};

impl Writer {
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // printable ASCII byte or newline
                0x20..=0x7e | b'\n' => unsafe{consputc(byte as i32 & 0xff)},
                // not part of printable ASCII range
                _ => unsafe{consputc(0xfe & 0xff)},
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// print! macro prints to the console

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::printf::_print(format_args!($($arg)*)));
}
/// println! macro prints to the console

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
/// Prints the given formatted string to the VGA text buffer
/// through the global WRITER instance.
// #[doc(hidden)]
pub unsafe fn _print(args: fmt::Arguments) {
    let mut locking: i32 = 0;
    locking = pr.locking;
    if locking != 0 {
        pr.lock.acquire();
    }

    use core::fmt::Write;
    
    WRITER.write_fmt(args).unwrap();
    
    if locking != 0 {
        pr.lock.release();
    };
}

impl PrintfLock {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
            locking: 0,
        }
    }
}

pub static mut panicked: i32 = 0;

static mut pr: PrintfLock = PrintfLock::zeroed();

static mut digits: [libc::c_char; 17] = [
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 0,
];

unsafe fn printint(mut xx: i32, mut base: i32, mut sign: i32) {
    let mut buf: [libc::c_char; 16] = [0; 16];
    let mut i: i32 = 0;
    let mut x: u32 = 0;
    if sign != 0 && {
        sign = (xx < 0) as i32;
        (sign) != 0
    } {
        x = -xx as u32
    } else {
        x = xx as u32
    }
    loop {
        let fresh0 = i;
        i += 1;
        buf[fresh0 as usize] = digits[x.wrapping_rem(base as u32) as usize];
        x = (x as u32).wrapping_div(base as u32) as u32 as u32;
        if x == 0 {
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
        if i < 0 {
            break;
        }
        consputc(buf[i as usize] as i32);
    }
}

unsafe fn printptr(mut x: usize) {
    consputc('0' as i32);
    consputc('x' as i32);
    for _i in 0..(::core::mem::size_of::<usize>()).wrapping_mul(2) {
        consputc(
            digits[(x
                >> (::core::mem::size_of::<usize>())
                    .wrapping_mul(8)
                    .wrapping_sub(4))] as i32,
        );
        x <<= 4
    }
}

/// Print to the console. only understands %d, %x, %p, %s.
pub unsafe extern "C" fn printf(mut fmt: *mut libc::c_char, mut args: ...) {
    let mut ap: ::core::ffi::VaListImpl;
    let mut i: i32 = 0;
    let mut locking: i32 = 0;
    locking = pr.locking;
    if locking != 0 {
        pr.lock.acquire();
    }
    if fmt.is_null() {
        panic(b"null fmt\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    ap = args.clone();
    loop {
        let mut c = *fmt.offset(i as isize) as i32 & 0xff;
        if c == 0 {
            break;
        }
        if c != '%' as i32 {
            consputc(c);
        } else {
            i += 1;
            c = *fmt.offset(i as isize) as i32 & 0xff;
            if c == 0 {
                break;
            }
            match c {
                100 => {
                    printint(ap.as_va_list().arg::<i32>(), 10, 1);
                }
                120 => {
                    printint(ap.as_va_list().arg::<i32>(), 16, 1);
                }
                112 => {
                    printptr(ap.as_va_list().arg::<usize>());
                }
                115 => {
                    let mut s = ap.as_va_list().arg::<*mut libc::c_char>();
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
        pr.lock.release();
    };
}

pub unsafe fn panic(mut s: *mut libc::c_char) -> ! {
    pr.locking = 0;
    printf(b"panic: \x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    printf(s);
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);

    // freeze other CPUs
    ::core::ptr::write_volatile(&mut panicked as *mut i32, 1);
    loop {}
}

pub unsafe fn printfinit() {
    pr.lock
        .initlock(b"pr\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    pr.locking = 1;
}
