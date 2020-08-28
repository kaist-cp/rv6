//! formatted console output -- printf, panic.
use crate::console::{LOCKING};
use crate::spinlock::RawSpinlock;
// use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// print! macro prints to the console
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// println! macro prints to the console
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe {
        LOCKING = true;
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

pub unsafe fn printfinit() {
    // PR.lock.initlock("PR");
    // PR.locking = 1;
}
