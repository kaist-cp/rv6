use crate::console::putc;
use core::fmt::{self, Write};

pub struct Printer {}

const BACKSPACE: i32 = 0x100;

impl Printer {
    pub const fn new() -> Self {
        Self {}
    }

    /// putc for Printer.
    pub fn putc(&mut self, c: i32) {
        putc(c);
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
