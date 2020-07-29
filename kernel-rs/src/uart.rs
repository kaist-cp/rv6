use crate::console::consoleintr;
use crate::libc;
use crate::memlayout::UART0;
use core::ptr;
/// low-level driver routines for 16550a UART.

/// the UART control registers are memory-mapped
/// at address UART0. this macro returns the
/// address of one of the registers.
/// the UART control registers.
/// some have different meanings for
/// read vs write.
/// http://byterunner.com/16550.html
/// receive holding register (for input bytes)
/// transmit holding register (for output bytes)
/// interrupt enable register
/// FIFO control register
/// interrupt status register
/// line control register
/// line status register
#[no_mangle]
pub unsafe extern "C" fn uartinit() {
    // disable interrupts.
    ptr::write_volatile((UART0 + 1 as i64) as *mut u8, 0 as libc::c_int as u8);
    // special mode to set baud rate.
    ptr::write_volatile((UART0 + 3 as i64) as *mut u8, 0x80 as libc::c_int as u8);
    // LSB for baud rate of 38.4K.
    ptr::write_volatile((UART0 + 0 as i64) as *mut u8, 0x3 as libc::c_int as u8);
    // MSB for baud rate of 38.4K.
    ptr::write_volatile((UART0 + 1 as i64) as *mut u8, 0 as libc::c_int as u8);
    // leave set-baud mode,
    // and set word length to 8 bits, no parity.
    ptr::write_volatile((UART0 + 3 as i64) as *mut u8, 0x3 as libc::c_int as u8);
    // reset and enable FIFOs.
    ptr::write_volatile((UART0 + 2 as i64) as *mut u8, 0x7 as libc::c_int as u8);
    // enable receive interrupts.
    ptr::write_volatile((UART0 + 1 as i64) as *mut u8, 0x1 as libc::c_int as u8);
}
/// write one output character to the UART.
#[no_mangle]
pub unsafe extern "C" fn uartputc(mut c: i32) {
    // wait for Transmit Holding Empty to be set in LSR.
    while ptr::read_volatile((UART0 + 5 as i64) as *mut u8) as i32 & (1 as i32) << 5 as i32
        == 0 as i32
    {}
    ptr::write_volatile((UART0 + 0 as i32 as i64) as *mut u8, c as u8);
}
/// read one input character from the UART.
/// return -1 if none is waiting.
#[no_mangle]
pub unsafe extern "C" fn uartgetc() -> i32 {
    if ptr::read_volatile((UART0 + 5 as i64) as *mut u8) as i32 & 0x1 as i32 != 0 {
        // input data is ready.
        ptr::read_volatile((UART0 + 0 as i64) as *mut u8) as i32
    } else {
        -1
    }
}
/// trap.c calls here when the uart interrupts.
#[no_mangle]
pub unsafe extern "C" fn uartintr() {
    loop {
        let mut c: i32 = uartgetc();
        if c == -(1 as i32) {
            break;
        }
        consoleintr(c);
    }
}
