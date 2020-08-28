//! formatted console output -- printf, panic.
use crate::console::CONS;
use crate::spinlock::RawSpinlock;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Writer {}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            unsafe {
                CONS.into_inner().putc(c as _);
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
    let _lock;
    if locking != true {
        _lock = CONS.lock();
    }
    (Writer {}).write_fmt(args).unwrap();
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe {
        locking = true;
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

pub static PANICKED: AtomicBool = AtomicBool::new(false);

// static mut PR: PrintfLock = PrintfLock::zeroed();
static mut locking: bool = false;

pub unsafe fn printfinit() {
    // PR.lock.initlock("PR");
    // PR.locking = 1;
}
