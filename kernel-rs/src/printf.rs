//! formatted console output -- println, panic.
use crate::console::{Console, CONS};
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// print! macro prints to the console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::printf::_print(format_args!($($arg)*)));
}

/// println! macro prints to the console.
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

    if LOCKING.load(Ordering::Acquire) != false {
        let mut lock = CONS.lock();
        lock.write_fmt(args).unwrap();
    } else {
        Console::zeroed().write_fmt(args).unwrap();
    }
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe {
        LOCKING.store(false, Ordering::Release);
        println!("{}", info);

        // Freeze other CPUs.
        PANICKED.store(true, Ordering::Release);
    }
    crate::utils::spin_loop()
}

pub static PANICKED: AtomicBool = AtomicBool::new(false);
pub static mut LOCKING: AtomicBool = AtomicBool::new(false);

pub unsafe fn printfinit() {
    LOCKING.store(true, Ordering::Release);
}
