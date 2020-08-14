//! formatted console output -- printf, panic.
use crate::console::consputc;
use crate::spinlock::RawSpinlock;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

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
        PANICKED.store(true, Ordering::Release);
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

pub static mut PANICKED: AtomicBool = AtomicBool::new(false);

static mut PR: PrintfLock = PrintfLock::zeroed();

pub unsafe fn printfinit() {
    PR.lock.initlock("PR");
    PR.locking = 1;
}
