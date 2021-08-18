//! Low-level driver routines for 16550a UART.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::{
    register_structs,
    registers::{ReadOnly, ReadWrite, WriteOnly},
};

register_structs! {
    /// The UART control registers.
    /// Some have different meanings for
    /// read vs write.
    /// see http://byterunner.com/16550.html
    #[allow(non_snake_case)]
    pub UartBlock {
        (0x00 => DR: ReadWrite<u32>),
        (0x04 => _reserved0),
        (0x18 => FR: ReadOnly<u32>),
        (0x1c => _reserved1),
        (0x24 => IBRD: WriteOnly<u32>),
        (0x28 => FBRD: WriteOnly<u32>),
        (0x2c => LCRH: WriteOnly<u32>),
        (0x30 => CR: WriteOnly<u32>),
        (0x34 => _reserved2),
        (0x38 => IMSC: WriteOnly<u32>),
        (0x44 => ICR: WriteOnly<u32>),
        (0x48 => @END),
    }
}

enum UartRegBits {
    FRTxFifoFull,  // tramit FIFO full
    FRRxFifoEmpty, // receive FIFO empty
    CRRxEnable,    // enable receive
    CRTxEnable,    // enable transmit
    CREnable,      // enable UART
    LCRFifoEnable, // enable FIFO
    IERTxEnable,   // transmit interrupt
    IERRxEnable,   // receive interrupt
}

pub const UART_CLK: usize = 24000000;
pub const UART_BITRATE: usize = 19200;

impl UartRegBits {
    fn bits(self) -> u32 {
        match self {
            UartRegBits::FRTxFifoFull | UartRegBits::IERTxEnable => 1 << 5,
            UartRegBits::FRRxFifoEmpty | UartRegBits::IERRxEnable | UartRegBits::LCRFifoEnable => {
                1 << 4
            }
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

/// # Safety
///
/// uart..(uart + 5) are owned addresses.
#[derive(Debug)]
pub struct Uart {
    uart: usize,
}

impl core::ops::Deref for Uart {
    type Target = UartBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl Uart {
    /// # Safety
    ///
    /// uart..(uart + 5) are owned addresses.
    pub const unsafe fn new(uart: usize) -> Self {
        Self { uart }
    }

    fn ptr(&self) -> *const UartBlock {
        self.uart as *const _
    }

    pub fn init(&self) {
        // set the bit rate: integer/fractional baud rate registers
        self.IBRD.set((UART_CLK / (16 * UART_BITRATE)) as u32);

        let left = UART_CLK % (16 * UART_BITRATE);
        self.FBRD
            .set(((left * 4 + UART_BITRATE / 2) / UART_BITRATE) as u32);

        // enable trasmit and receive interrupts
        self.CR.set(
            UartRegBits::CREnable.bits()
                | UartRegBits::CRRxEnable.bits()
                | UartRegBits::CRTxEnable.bits(),
        );

        self.LCRH.set(UartRegBits::LCRFifoEnable.bits());

        self.IMSC.set(UartRegBits::IERRxEnable.bits());
    }

    pub fn enable_rx(&self) {
        self.IMSC.set(UartRegBits::IERRxEnable.bits());
    }

    /// Read one input character from the UART. Return Err(()) if none is waiting.
    pub fn getc(&self) -> Result<i32, ()> {
        if self.FR.get() & UartRegBits::FRRxFifoEmpty.bits() == 0 {
            // Input data is ready.
            Ok(self.DR.get() as i32)
        } else {
            Err(())
        }
    }

    /// Write one output character to the UART.
    pub fn putc(&self, c: u8) {
        self.DR.set(c.into());
    }

    /// Check whether the UART transmit holding register is full.
    pub fn is_full(&self) -> bool {
        (self.FR.get() & UartRegBits::FRTxFifoFull.bits()) == 1
    }

    pub fn puts(&self, s: &str) {
        for c in s.chars() {
            self.putc(c as u8);
        }
    }
}
