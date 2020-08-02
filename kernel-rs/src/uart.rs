//! low-level driver routines for 16550a UART.

use crate::console::consoleintr;
use crate::memlayout::UART0;
use core::ptr;

/// the UART control registers are memory-mapped
/// at address UART0. this macro returns the
/// address of one of the registers.
const fn reg(r: i32) -> *mut u8 {
    (UART0 + r as u64) as *mut u8
}

/// the UART control registers.
/// some have different meanings for
/// read vs write.
/// http://byterunner.com/16550.html

/// receive holding register (for input bytes)
const RHR: i32 = 0;

/// transmit holding register (for output bytes)
const THR: i32 = 0;

/// interrupt enable register
const IER: i32 = 1;

/// FIFO control register
const FCR: i32 = 2;

/// interrupt status register
const ISR: i32 = 2;

/// line control register
const LCR: i32 = 3;

/// line status register
const LSR: i32 = 5;

const fn read_reg(r: i32) -> *mut u8 {
    reg(r) as *mut u8
}
unsafe fn write_reg(r: i32, v: i32) {
    ptr::write_volatile(reg(r), v as u8)
}

pub unsafe fn uartinit() {
    // disable interrupts.
    write_reg(IER, 0x00);
    // special mode to set baud rate.
    write_reg(LCR, 0x80);
    // LSB for baud rate of 38.4K.
    write_reg(0, 0x03);
    // MSB for baud rate of 38.4K.
    write_reg(1, 0x00);
    // leave set-baud mode,
    // and set word length to 8 bits, no parity.
    write_reg(LCR, 0x03);
    // reset and enable FIFOs.
    write_reg(FCR, 0x07);
    // enable receive interrupts.
    write_reg(IER, 0x01);
}

/// write one output character to the UART.
pub unsafe fn uartputc(mut c: i32) {
    // wait for Transmit Holding Empty to be set in LSR.
    while ptr::read_volatile(read_reg(LSR)) as i32 & 1 << 5 == 0 {}
    write_reg(THR, c);
}

/// read one input character from the UART.
/// return -1 if none is waiting.
pub unsafe fn uartgetc() -> i32 {
    if ptr::read_volatile(read_reg(LSR)) as i32 & 0x1 != 0 {
        // input data is ready.
        ptr::read_volatile(read_reg(RHR)) as i32
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
