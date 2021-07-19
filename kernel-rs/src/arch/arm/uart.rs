//! Low-level driver routines for 16550a UART.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use core::ptr;

use self::UartCtrlRegs::*;

enum UartRegBits {
    FRTxFifoFull, // tramit FIFO full
    FRRxFifoEmpty, // receive FIFO empty
    CRRxEnable, // enable receive
    CRTxEnable, // enable transmit
    CREnable, // enable UART
    LCRFifoEnable, // enable FIFO
    IERTxEnable, // transmit interrupt
    IERRxEnable, // receive interrupt
}

pub const UART_CLK: usize = 24000000;
pub const UART_BITRATE: usize = 19200;

impl UartRegBits {
    fn bits(self) -> u16 {
        match self {
            UartRegBits::FRTxFifoFull | UartRegBits::IERTxEnable => 1 << 5,
            UartRegBits::FRRxFifoEmpty | UartRegBits::IERRxEnable 
            | UartRegBits::LCRFifoEnable => 1 << 4,
            UartRegBits::CRRxEnable => 1 << 9,
            UartRegBits::CRTxEnable => 1 << 8,
            UartRegBits::CREnable => 1 << 0,
        }
    }
}

/// The UART control registers.
/// Some have different meanings for
/// read vs write.
/// see http://byterunner.com/16550.html
#[repr(usize)]
enum UartCtrlRegs {
    /// Data Register.
    DR,
    /// Receive Status Register/error clear Register.
    RSR,
    /// Flag Register.
    FR,
    /// Integer Baud Rate Register.
    IBRD,
    /// Fractional Baud Rate Register.
    FBRD,
    /// Line Control Register.
    LCR,
    /// Control Register.
    CR,
    /// Interrupt Mask Set/Clear Register
    IMSC,
    /// Masked Interrupt Status Register
    MIS,
    /// Interrupt Clear Register
    ICR,
}

impl UartCtrlRegs {
    /// The UART control registers are memory-mapped
    /// at address uart. This macro returns the
    /// address of one of the registers.
    fn addr(self, uart: usize) -> *mut u16 {
        match self {
            DR => uart as *mut u16,
            RSR => (uart + 1) as *mut u16,
            FR => (uart + 6) as  *mut u16,
            IBRD => (uart + 9) as  *mut u16,
            FBRD => (uart + 10) as  *mut u16,
            LCR => (uart + 11) as  *mut u16,
            CR => (uart + 12) as  *mut u16,
            IMSC => (uart + 14) as  *mut u16,
            MIS => (uart + 16) as  *mut u16,
            ICR => (uart + 17) as  *mut u16,
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
        // set the bit rate: integer/fractional baud rate registers
        self.write(IBRD, (UART_CLK / (16 * UART_BITRATE)) as u16);

        let left = UART_CLK % (16 * UART_BITRATE);
        self.write(FBRD, ((left * 4 + UART_BITRATE / 2) / UART_BITRATE) as u16);

        // enable trasmit and receive
        self.write(CR, UartRegBits::CREnable.bits() | UartRegBits::CRRxEnable.bits() | UartRegBits::CRTxEnable.bits());

        // enable FIFO
        self.write(LCR, UartRegBits::LCRFifoEnable.bits());

        // // Special mode to set baud rate.
        // self.write(LCR, UartRegBits::LCRBaudLatch.bits());

        // // LSB for baud rate of 38.4K.
        // self.write(RBR, 0x03);

        // // MSB for baud rate of 38.4K.
        // self.write(IER, 0x00);

        // // Leave set-baud mode,
        // // and set word length to 8 bits, no parity.
        // self.write(LCR, UartRegBits::LCREightBits.bits());

        // // Reset and enable FIFOs.
        // self.write(
        //     FCR,
        //     UartRegBits::FCRFifoEnable.bits() | UartRegBits::FCRFifoClear.bits(),
        // );

        // // Enable transmit and receive interrupts.
        // self.write(
        //     IER,
        //     UartRegBits::IERTxEnable.bits() | UartRegBits::IERRxEnable.bits(),
        // );
    }

    /// Read one input character from the UART. Return Err(()) if none is waiting.
    pub fn getc(&self) -> Result<i32, ()> {
        if self.read(FR) & UartRegBits::FRRxFifoEmpty.bits() != 0 {
            // Input data is ready.
            Ok(self.read(DR) as i32)
        } else {
            Err(())
        }
    }

    /// Write one output character to the UART.
    pub fn putc(&self, c: u8) {
        self.write(DR, c.into());
    }

    /// Check whether the UART transmit holding register is full.
    pub fn is_full(&self) -> bool {
        (self.read(FR) & UartRegBits::FRTxFifoFull.bits()) == 1
    }

    fn read(&self, reg: UartCtrlRegs) -> u16 {
        // SAFETY:
        // * the address is valid because of the invariant of self.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::read_volatile(reg.addr(self.uart)) }
    }

    fn write(&self, reg: UartCtrlRegs, v: u16) {
        // SAFETY:
        // * the address is valid because of the invariant of self.
        // * volatile concurrent accesses are safe.
        //   (https://github.com/kaist-cp/rv6/issues/188#issuecomment-683548362)
        unsafe { ptr::write_volatile(reg.addr(self.uart), v) }
    }
}
