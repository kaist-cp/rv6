//! formatted console output -- printf, panic.
use crate::console::Console;
use crate::spinlock::Spinlock;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Writer {}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            unsafe {
                Console::putc(c as _);
            }
        }
        Ok(())
    }
}

/// Lock to avoid interleaving concurrent printf's.
static WRITER: Spinlock<Writer> = Spinlock::new("WRITER", Writer {});

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
pub fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;
    if LOCKING.load(Ordering::SeqCst) {
        let mut writer = WRITER.lock();
        writer.write_fmt(args).unwrap();
    }
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    {
        LOCKING.store(false, Ordering::Release);
        println!("{}", info);

        // freeze other CPUs
        PANICKED.store(true, Ordering::Release);
    }
    crate::utils::spin_loop()
}

/// TODO: Need appropriate comment.
pub static PANICKED: AtomicBool = AtomicBool::new(false);
pub static LOCKING: AtomicBool = AtomicBool::new(true);

pub fn printfinit() {
    LOCKING.store(true, Ordering::Release);
}
