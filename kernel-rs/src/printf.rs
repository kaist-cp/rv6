//! formatted console output -- printf, panic.
use crate::console::consputc;
use crate::spinlock::RawSpinlock;
use core::fmt;

pub struct Writer {}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            unsafe {
                consputc(c as _);
            }
        }
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
#[doc(hidden)]
pub unsafe fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;
    let locking: i32 = PR.locking;
    if locking != 0 {
        PR.lock.acquire();
    }
    (Writer {}).write_fmt(args).unwrap();
    if locking != 0 {
        PR.lock.release();
    }
}

/// lock to avoid interleaving concurrent printf's.
struct PrintfLock {
    lock: RawSpinlock,
    locking: i32,
}

impl PrintfLock {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            locking: 0,
        }
    }
}

pub static mut PANICKED: i32 = 0;

static mut PR: PrintfLock = PrintfLock::zeroed();

static mut DIGITS: [u8; 17] = [
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 0,
];

unsafe fn printint(xx: i32, base: i32, mut sign: i32) {
    let mut buf: [u8; 16] = [0; 16];
    let mut i: i32 = 0;
    let mut x: u32 = if sign != 0 && {
        sign = (xx < 0) as i32;
        (sign) != 0
    } {
        -xx as u32
    } else {
        xx as u32
    };
    loop {
        let fresh0 = i;
        i += 1;
        buf[fresh0 as usize] = DIGITS[x.wrapping_rem(base as u32) as usize];
        x = (x as u32).wrapping_div(base as u32) as u32 as u32;
        if x == 0 {
            break;
        }
    }
    if sign != 0 {
        let fresh1 = i;
        i += 1;
        buf[fresh1 as usize] = b'-'
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
            DIGITS[(x
                >> (::core::mem::size_of::<usize>())
                    .wrapping_mul(8)
                    .wrapping_sub(4))] as i32,
        );
        x <<= 4
    }
}

/// Print to the console. only understands %d, %x, %p, %s.
pub unsafe extern "C" fn printf(fmt: *const u8, args: ...) {
    let mut ap: ::core::ffi::VaListImpl<'_>;
    let mut i: i32 = 0;
    let locking: i32 = PR.locking;
    if locking != 0 {
        PR.lock.acquire();
    }
    if fmt.is_null() {
        panic(b"null fmt\x00" as *const u8 as *mut u8);
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
                    let mut s = ap.as_va_list().arg::<*mut u8>();
                    if s.is_null() {
                        s = b"(null)\x00" as *const u8 as *mut u8
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
        PR.lock.release();
    }
}

pub unsafe fn panic(s: *mut u8) -> ! {
    PR.locking = 0;
    print!("panic: ");
    printf(s);
    println!();

    // freeze other CPUs
    ::core::ptr::write_volatile(&mut PANICKED as *mut i32, 1);
    loop {}
}

pub unsafe fn printfinit() {
    PR.lock.initlock(b"PR\x00" as *const u8 as *mut u8);
    PR.locking = 1;
}
