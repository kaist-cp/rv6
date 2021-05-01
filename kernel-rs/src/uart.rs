//! Low-level driver routines for 16550a UART.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use core::ptr;

use self::UartCtrlRegs::{FCR, IER, ISR, LCR, LSR, RBR, THR};

enum UartRegBits {
    IERTxEnable,
    IERRxEnable,
    FCRFifoEnable,
    FCRFifoClear,
    LCREightBits,
    LCRBaudLatch,
    LSRRxRead,
    LSRTxIdle,
}

impl UartRegBits {
    fn bits(self) -> u8 {
        match self {
            UartRegBits::FCRFifoEnable | UartRegBits::IERRxEnable
            // Input is waiting to be read from RHR.
            | UartRegBits::LSRRxRead => 1 << 0,
            UartRegBits::IERTxEnable => 1 << 1,
            // Clear the content of the two FIFOs.
            UartRegBits::FCRFifoClear => 3 << 1,
            UartRegBits::LCREightBits => 3,
            // Special mode to set baud rate.
            UartRegBits::LCRBaudLatch => 1 << 7,
            // THR can accept another character to send.
            UartRegBits::LSRTxIdle => 1 << 5,
        }
    }
}

/// The UART control registers.
/// Some have different meanings for
/// read vs write.
/// see http://byterunner.com/16550.html
#[repr(usize)]
enum UartCtrlRegs {
    /// Recieve Buffer Register.
    RBR,
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
}

impl UartCtrlRegs {
    /// The UART control registers are memory-mapped
    /// at address uart. This macro returns the
    /// address of one of the registers.
    fn addr(self, uart: usize) -> *mut u8 {
        match self {
            THR | RBR => uart as *mut u8,
            IER => (uart + 1) as *mut u8,
            FCR | ISR => (uart + 2) as *mut u8,
            LCR => (uart + 3) as *mut u8,
            LSR => (uart + 5) as *mut u8,
        }
    }
}

/// # Safety
///
/// uart..(uart + 5) are owned addresses.
pub struct Uart {
    uart: usize,
}

impl Uart {
    /// # Safety
    ///
    /// uart..(uart + 5) are owned addresses.
    pub const unsafe fn new(uart: usize) -> Self {
        Self { uart }
    }

    pub fn init(&self) {
        // Disable interrupts.
        self.write(IER, 0x00);

        // Special mode to set baud rate.
        self.write(LCR, UartRegBits::LCRBaudLatch.bits());

        // LSB for baud rate of 38.4K.
        self.write(RBR, 0x03);

        // MSB for baud rate of 38.4K.
        self.write(IER, 0x00);

        // Leave set-baud mode,
        // and set word length to 8 bits, no parity.
        self.write(LCR, UartRegBits::LCREightBits.bits());

        // Reset and enable FIFOs.
        self.write(
            FCR,
            UartRegBits::FCRFifoEnable.bits() | UartRegBits::FCRFifoClear.bits(),
        );

        // Enable transmit and receive interrupts.
        self.write(
            IER,
            UartRegBits::IERTxEnable.bits() | UartRegBits::IERRxEnable.bits(),
        );
    }

    /// Read one input character from the UART. Return Err(()) if none is waiting.
    pub fn getc(&self) -> Result<i32, ()> {
        if self.read(LSR) & 0x01 != 0 {
            // Input data is ready.
            Ok(self.read(RBR) as i32)
        } else {
            Err(())
        }
    }

    /// Write one output character to the UART.
    pub fn putc(&self, c: u8) {
        self.write(THR, c);
    }

    /// Check whether the UART transmit holding register is full.
    pub fn is_full(&self) -> bool {
        (self.read(LSR) & UartRegBits::LSRTxIdle.bits()) == 0
    }

    fn read(&self, reg: UartCtrlRegs) -> u8 {
        // SAFETY:
        // * the address is valid because of the invariant of self.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::read_volatile(reg.addr(self.uart)) }
    }

    fn write(&self, reg: UartCtrlRegs, v: u8) {
        // SAFETY:
        // * the address is valid because of the invariant of self.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::write_volatile(reg.addr(self.uart), v) }
    }
}
