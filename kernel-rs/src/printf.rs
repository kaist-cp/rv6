//! formatted console output -- println.
use crate::kernel::kernel;
use core::fmt;

/// Prints the given formatted string with the Console.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;

    if !kernel().is_panicked() {
        let mut lock = kernel().console.lock();
        lock.write_fmt(args).unwrap();
    } else {
        unsafe {
            kernel()
                .console
                .get_mut_unchecked()
                .write_fmt(args)
                .unwrap();
        }
    }
}

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
