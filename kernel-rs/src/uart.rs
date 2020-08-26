//! low-level driver routines for 16550a UART.
use crate::console::consoleintr;
use crate::memlayout::UART0;
use core::ptr;

use self::UartCtrlRegs::{FCR, IER, ISR, LCR, LSB, LSR, MSB, RHR, THR};

/// The UART control registers.
/// Some have different meanings for
/// read vs write.
/// http://byterunner.com/16550.html
#[repr(usize)]
enum UartCtrlRegs {
    /// Receive Holding Register (for input bytes).
    RHR,
    /// Transmit Holding Register (for output bytes).
    THR,
    /// Interrupt Enable Register.
    IER,
    /// FIFO Control Register.
    FCR,
    /// Interrupt Status Register.
    ISR,
    /// Line Control Register.
    LCR,
    /// Line Status Register.
    LSR,
    /// LSB for baud rate of 38.4K.
    LSB,
    /// MSB for baud rate of 38.4K.
    MSB,
}

impl UartCtrlRegs {
    /// The UART control registers are memory-mapped
    /// at address UART0. This macro returns the
    /// address of one of the registers.
    unsafe fn reg(self) -> *mut u8 {
        match self {
            RHR | THR | LSB => UART0 as *mut u8,
            IER | MSB => (UART0 +1) as *mut u8,
            FCR | ISR => (UART0 +2) as *mut u8,
            LCR => (UART0 +3) as *mut u8,
            LSR => (UART0 +5) as *mut u8,
        }
    }

    fn read(self) -> u8 {
        unsafe { ptr::read_volatile(self.reg()) }
    }

    fn write(self, v: u8) {
        unsafe { ptr::write_volatile(self.reg(), v) }
    }
}

pub unsafe fn uartinit() {
    // Disable interrupts.
    IER.write(0x00);

    // Special mode to set baud rate.
    LCR.write(0x80);

    // LSB for baud rate of 38.4K.
    LSB.write(0x03);

    // MSB for baud rate of 38.4K.
    MSB.write(0x00);

    // Leave set-baud mode,
    // and set word length to 8 bits, no parity.
    LCR.write(0x03);

    // Reset and enable FIFOs.
    FCR.write(0x07);

    // Enable receive interrupts.
    IER.write(0x01);
}

/// Write one output character to the UART.
pub fn uartputc(c: i32) {
    // Wait for Transmit Holding Empty to be set in LSR.
    while LSR.read() & 1 << 5 == 0 {}
    THR.write(c as u8);
}

/// Read one input character from the UART.
/// Return -1 if none is waiting.
fn uartgetc() -> i32 {
    if LSR.read() & 0x01 != 0 {
        // Input data is ready.
        RHR.read() as i32
    } else {
        -1
    }
}

/// trap.c calls here when the uart interrupts.
pub fn uartintr() {
    loop {
        let c = uartgetc();
        if c == -1 {
            break;
        }
        unsafe { consoleintr(c); }
    }
}
