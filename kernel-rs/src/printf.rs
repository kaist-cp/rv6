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

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe {
        PR.locking = 0;
        println!("{}", info);

        // freeze other CPUs
        ::core::ptr::write_volatile(&mut PANICKED as *mut i32, 1);
    }
    crate::utils::spin_loop()
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

pub unsafe fn printfinit() {
    PR.lock.initlock(b"PR\x00" as *const u8 as *mut u8);
    PR.locking = 1;
}
