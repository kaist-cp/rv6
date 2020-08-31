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

/// Prints the given formatted string with the Console.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;

    if LOCKING.load(Ordering::Relaxed) {
        let mut lock = CONS.lock();
        lock.write_fmt(args).unwrap();
    } else {
        // TODO: Need to find another method or change the function name when modifying the 'zeroed()' part.
        Console::zeroed().write_fmt(args).unwrap();
    }
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    LOCKING.store(false, Ordering::Relaxed);
    println!("{}", info);

    // Freeze other CPUs.
    PANICKED.store(true, Ordering::Release);

    crate::utils::spin_loop()
}

pub static PANICKED: AtomicBool = AtomicBool::new(false);
pub static LOCKING: AtomicBool = AtomicBool::new(false);

pub fn printfinit() {
    LOCKING.store(true, Ordering::Release);
}
