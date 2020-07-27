use crate::console::consoleintr;
use crate::libc;
use core::ptr;
// Physical memory layout
// qemu -machine virt is set up like this,
// based on qemu's hw/riscv/virt.c:
//
// 00001000 -- boot ROM, provided by qemu
// 02000000 -- CLINT
// 0C000000 -- PLIC
// 10000000 -- uart0
// 10001000 -- virtio disk
// 80000000 -- boot ROM jumps here in machine mode
//             -kernel loads the kernel here
// unused RAM after 80000000.
// the kernel uses physical memory thus:
// 80000000 -- entry.S, then kernel text and data
// end -- start of kernel page allocation area
// PHYSTOP -- end RAM used by the kernel
// qemu puts UART registers here in physical memory.
pub const UART0: libc::c_long = 0x10000000 as libc::c_long;

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
    ptr::write_volatile(
        (UART0 + 1 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0 as libc::c_int as libc::c_uchar,
    );
    // special mode to set baud rate.
    ptr::write_volatile(
        (UART0 + 3 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0x80 as libc::c_int as libc::c_uchar,
    );
    // LSB for baud rate of 38.4K.
    ptr::write_volatile(
        (UART0 + 0 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0x3 as libc::c_int as libc::c_uchar,
    );
    // MSB for baud rate of 38.4K.
    ptr::write_volatile(
        (UART0 + 1 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0 as libc::c_int as libc::c_uchar,
    );
    // leave set-baud mode,
    // and set word length to 8 bits, no parity.
    ptr::write_volatile(
        (UART0 + 3 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0x3 as libc::c_int as libc::c_uchar,
    );
    // reset and enable FIFOs.
    ptr::write_volatile(
        (UART0 + 2 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0x7 as libc::c_int as libc::c_uchar,
    );
    // enable receive interrupts.
    ptr::write_volatile(
        (UART0 + 1 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        0x1 as libc::c_int as libc::c_uchar,
    );
}
/// write one output character to the UART.
#[no_mangle]
pub unsafe extern "C" fn uartputc(mut c: libc::c_int) {
    // wait for Transmit Holding Empty to be set in LSR.
    while ptr::read_volatile((UART0 + 5 as libc::c_int as libc::c_long) as *mut libc::c_uchar)
        as libc::c_int
        & (1 as libc::c_int) << 5 as libc::c_int
        == 0 as libc::c_int
    {}
    ptr::write_volatile(
        (UART0 + 0 as libc::c_int as libc::c_long) as *mut libc::c_uchar,
        c as libc::c_uchar,
    );
}
/// read one input character from the UART.
/// return -1 if none is waiting.
#[no_mangle]
pub unsafe extern "C" fn uartgetc() -> libc::c_int {
    if ptr::read_volatile((UART0 + 5 as libc::c_int as libc::c_long) as *mut libc::c_uchar)
        as libc::c_int
        & 0x1 as libc::c_int
        != 0
    {
        // input data is ready.
        ptr::read_volatile((UART0 + 0 as libc::c_int as libc::c_long) as *mut libc::c_uchar)
            as libc::c_int
    } else {
        -(1 as libc::c_int)
    }
}
/// trap.c calls here when the uart interrupts.
#[no_mangle]
pub unsafe extern "C" fn uartintr() {
    loop {
        let mut c: libc::c_int = uartgetc();
        if c == -(1 as libc::c_int) {
            break;
        }
        consoleintr(c);
    }
}
