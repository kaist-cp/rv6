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
    /// LSB for baud rate.
    LSB,
    /// MSB for baud rate.
    MSB,
}

/// the UART control registers are memory-mapped
/// at address UART0. this macro returns the
/// address of one of the registers.
impl UartCtrlRegs {
    unsafe fn reg(self) -> *mut u8 {
        match self {
            RHR | THR | LSB => (UART0 as *mut u8).add(0 as _),
            IER | MSB => (UART0 as *mut u8).add(1 as _),
            FCR | ISR => (UART0 as *mut u8).add(2 as _),
            LCR => (UART0 as *mut u8).add(3 as _),
            LSR => (UART0 as *mut u8).add(5 as _),
        }
    }

    unsafe fn read(self) -> u8 {
        ptr::read_volatile(self.reg())
    }

    unsafe fn write(self, v: u8) {
        ptr::write_volatile(self.reg(), v)
    }
}

pub unsafe fn uartinit() {
    // disable interrupts.
    IER.write(0x00);

    // special mode to set baud rate.
    LCR.write(0x80);

    // LSB for baud rate of 38.4K.
    LSB.write(0x03);

    // MSB for baud rate of 38.4K.
    MSB.write(0x00);

    // leave set-baud mode,
    // and set word length to 8 bits, no parity.
    LCR.write(0x03);

    // reset and enable FIFOs.
    FCR.write(0x07);

    // enable receive interrupts.
    IER.write(0x01);
}

/// write one output character to the UART.
pub unsafe fn uartputc(c: i32) {
    // wait for Transmit Holding Empty to be set in LSR.
    while LSR.read() & 1 << 5 == 0 {}
    THR.write(c as u8);
}

/// read one input character from the UART.
/// return -1 if none is waiting.
pub unsafe fn uartgetc() -> i32 {
    if LSR.read() & 0x1 != 0 {
        // input data is ready.
        RHR.read() as i32
    } else {
        -1
    }
}

/// trap.c calls here when the uart interrupts.
pub unsafe fn uartintr() {
    loop {
        let c = uartgetc();
        if c == -1 {
            break;
        }
        consoleintr(c);
    }
}
