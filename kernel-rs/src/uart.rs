//! Low-level driver routines for 16550a UART.
use core::ptr;

use self::UartCtrlRegs::{FCR, IER, ISR, LCR, LSR, RBR, THR};
use crate::memlayout::UART0;
use crate::{
    console::consoleintr,
    kernel::kernel_builder,
    lock::{pop_off, push_off},
    sleepablelock::{Sleepablelock, SleepablelockGuard},
    utils::spin_loop,
};

const UART_TX_BUF_SIZE: usize = 32;

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
    /// at address UART0. This macro returns the
    /// address of one of the registers.
    fn reg(self) -> *mut u8 {
        match self {
            THR | RBR => UART0 as *mut u8,
            IER => (UART0 + 1) as *mut u8,
            FCR | ISR => (UART0 + 2) as *mut u8,
            LCR => (UART0 + 3) as *mut u8,
            LSR => (UART0 + 5) as *mut u8,
        }
    }

    fn read(self) -> u8 {
        unsafe { ptr::read_volatile(self.reg()) }
    }

    fn write(self, v: u8) {
        unsafe { ptr::write_volatile(self.reg(), v) }
    }
}

pub struct UartTX {
    pub buf: [u8; UART_TX_BUF_SIZE],

    /// Write next to uart_tx_buf[uart_tx_w % UART_TX_BUF_SIZE]
    pub w: u64,

    /// Read next from uart_tx_buf[uar_tx_r % UART_TX_BUF_SIZE]
    pub r: u64,
}

pub struct Uart {
    pub tx_lock: Sleepablelock<UartTX>,
}

impl Uart {
    pub const fn new() -> Self {
        Self {
            tx_lock: Sleepablelock::new(
                "uart",
                UartTX {
                    buf: [0; UART_TX_BUF_SIZE],
                    w: 0,
                    r: 0,
                },
            ),
        }
    }

    pub fn init() {
        // Disable interrupts.
        IER.write(0x00);

        // Special mode to set baud rate.
        LCR.write(UartRegBits::LCRBaudLatch.bits());

        // LSB for baud rate of 38.4K.
        RBR.write(0x03);

        // MSB for baud rate of 38.4K.
        IER.write(0x00);

        // Leave set-baud mode,
        // and set word length to 8 bits, no parity.
        LCR.write(UartRegBits::LCREightBits.bits());

        // Reset and enable FIFOs.
        FCR.write(UartRegBits::FCRFifoEnable.bits() | UartRegBits::FCRFifoClear.bits());

        // Enable transmit and receive interrupts.
        IER.write(UartRegBits::IERTxEnable.bits() | UartRegBits::IERRxEnable.bits());
    }

    /// Add a character to the output buffer and tell the
    /// UART to start sending if it isn't already.
    /// Blocks if the output buffer is full.
    /// Since it may block, it can't be called
    /// from interrupts; it's only suitable for use
    /// by write().
    pub fn putc(&self, c: i32) {
        let mut guard = self.tx_lock.lock();
        if kernel_builder().is_panicked() {
            spin_loop();
        }
        loop {
            if guard.w == guard.r + UART_TX_BUF_SIZE as u64 {
                // Buffer is full.
                // Wait for uartstart() to open up space in the buffer.
                guard.sleep();
            } else {
                let w = guard.w;
                guard.buf[w as usize % UART_TX_BUF_SIZE] = c as u8;
                guard.w += 1;
                self.start(guard);
                return;
            }
        }
    }

    /// Alternate version of uartputc() that doesn't
    /// use interrupts, for use by kernel printf() and
    /// to echo characters. It spins waiting for the uart's
    /// output register to be empty.
    pub fn putc_sync(c: i32) {
        unsafe {
            push_off();
        }
        if kernel_builder().is_panicked() {
            spin_loop();
        }

        // Wait for Transmit Holding Empty to be set in LSR.
        while LSR.read() & UartRegBits::LSRTxIdle.bits() == 0 {}

        THR.write(c as u8);

        unsafe {
            pop_off();
        }
    }

    /// If the UART is idle, and a character is waiting
    /// in the transmit buffer, send it.
    /// Caller must hold uart_tx_lock.
    /// Called from both the top- and bottom-half.
    fn start(&self, mut guard: SleepablelockGuard<'_, UartTX>) {
        loop {
            if guard.w == guard.r {
                // Transmit buffer is empty.
                return;
            }

            if (LSR.read() & UartRegBits::LSRTxIdle.bits()) == 0 {
                // The UART transmit holding register is full,
                // so we cannot give it another byte.
                // It will interrupt when it's ready for a new byte.
                return;
            }

            let c = guard.buf[guard.r as usize % UART_TX_BUF_SIZE];
            guard.r += 1;

            // Maybe uartputc() is waiting for space in the buffer.
            guard.wakeup();

            THR.write(c);
        }
    }

    /// Read one input character from the UART.
    /// Return -1 if none is waiting.
    /// TODO(https://github.com/kaist-cp/rv6/issues/361)
    /// should get &self - need to refactor when encapsulate Uart into Console.
    fn getc() -> i32 {
        if LSR.read() & 0x01 != 0 {
            // Input data is ready.
            RBR.read() as i32
        } else {
            -1
        }
    }

    /// Handle a uart interrupt, raised because input has
    /// arrived, or the uart is ready for more output, or
    /// both. Called from trap.c.
    pub fn intr(&self) {
        // Read and process incoming characters.
        loop {
            let c = Uart::getc();
            if c == -1 {
                break;
            }
            unsafe {
                consoleintr(c);
            }
        }

        // Send buffered characters.
        self.start(self.tx_lock.lock());
    }
}
