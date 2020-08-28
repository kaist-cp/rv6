//! formatted console output -- println, panic.
use crate::console::Console;
use crate::spinlock::Spinlock;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

struct Writer {}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            {
                Console::putc(c as _);
            }
        }
        Ok(())
    }
}

/// The global WRITER.
/// Lock to avoid interleaving concurrent printf's.
static WRITER: Spinlock<Writer> = Spinlock::new("WRITER", Writer {});

/// TODO: Need appropriate comments.
pub static PANICKED: AtomicBool = AtomicBool::new(false);
static LOCKING: AtomicBool = AtomicBool::new(true);

/// TODO: Need appropriate comments.
pub fn printfinit() {
    LOCKING.store(true, Ordering::SeqCst);
}

/// Prints the given formatted string with the global WRITER.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    use core::fmt::Write;
    if LOCKING.load(Ordering::SeqCst) {
        let mut writer = WRITER.lock();
        writer.write_fmt(args).unwrap();
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

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    {
        LOCKING.store(false, Ordering::SeqCst);
        println!("{}", info);

        // Freeze other CPUs.
        PANICKED.store(true, Ordering::Release);
    }
    crate::utils::spin_loop()
}
