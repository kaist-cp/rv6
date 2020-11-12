use crate::kernel::kernel;
use crate::uart::Uart;
use crate::utils::spin_loop;
use core::fmt::{self, Write};

pub struct Printer {
    uart: Uart,
}

const BACKSPACE: i32 = 0x100;

impl Printer {
    pub const fn new() -> Self {
        Self { uart: Uart::new() }
    }

    /// Send one character to the uart.
    pub fn putc(&mut self, c: i32) {
        // From printf.rs.
        if kernel().is_panicked() {
            spin_loop();
        }
        if c == BACKSPACE {
            // If the user typed backspace, overwrite with a space.
            self.uart.putc('\u{8}' as i32, false);
            self.uart.putc(' ' as i32, false);
            self.uart.putc('\u{8}' as i32, false);
        } else {
            self.uart.putc(c, false);
        };
    }
}

impl Write for Printer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            self.putc(c as _);
        }
        Ok(())
    }
}
