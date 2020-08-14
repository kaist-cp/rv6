//! formatted console output -- printf, panic.
use crate::console::consputc;
use crate::spinlock::Spinlock;
use core::{
    fmt, mem,
    ops::{Deref, DerefMut},
};

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
    let locking = PR.locking.lock();
    if *(locking.deref()) == 0 {
        drop(locking)
    }
    (Writer {}).write_fmt(args).unwrap();
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe {
        let _ = mem::replace(PR.locking.lock().deref_mut(), 1);
        println!("{}", info);

        // freeze other CPUs
        ::core::ptr::write_volatile(&mut PANICKED as *mut i32, 1);
    }
    crate::utils::spin_loop()
}

/// lock to avoid interleaving concurrent printf's.
struct PrintfLock {
    locking: Spinlock<i32>,
}

impl PrintfLock {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            locking: Spinlock::new("PR", 0),
        }
    }
}

pub static mut PANICKED: i32 = 0;

static mut PR: PrintfLock = PrintfLock::zeroed();

pub unsafe fn printfinit() {
    let _ = mem::replace(PR.locking.lock().deref_mut(), 1);
}
