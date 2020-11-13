//! low-level driver routines for 16550a UART.
use crate::memlayout::UART0;
use crate::{
    console::consoleintr,
    sleepablelock::{Sleepablelock, SleepablelockGuard},
};
use core::ptr;

use self::UartCtrlRegs::{FCR, IER, ISR, LCR, LSR, RBR, THR};

const UART_TX_BUF_SIZE: usize = 32;

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

    /// write next to uart_tx_buf[uart_tx_w++]
    pub w: i32,

    /// read next from uart_tx_buf[uar_tx_r++]
    pub r: i32,
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
        LCR.write(0x80);

        // LSB for baud rate of 38.4K.
        RBR.write(0x03);

        // MSB for baud rate of 38.4K.
        IER.write(0x00);

        // Leave set-baud mode,
        // and set word length to 8 bits, no parity.
        LCR.write(0x03);

        // Reset and enable FIFOs.
        FCR.write(0x07);

        // Enable transmit and receive interrupts.
        IER.write(0x02 | 0x01);
    }

    /// add a character to the output buffer and tell the
    /// UART to start sending if it isn't already.
    ///
    /// usually called from the top-half -- by a process
    /// calling write(). can also be called from a uart
    /// interrupt to echo a received character, or by printf
    /// or panic from anywhere in the kernel.
    ///
    /// the block argument controls what happens if the
    /// buffer is full. for write(), block is 1, and the
    /// process waits. for kernel printf's and echoed
    /// characters, block is 0, and the character is
    /// discarded; this is necessary since sleep() is
    /// not possible in interrupts.
    pub fn putc(&self, c: i32, block: bool) {
        let mut guard = self.tx_lock.lock();
        loop {
            if (guard.w + 1) % UART_TX_BUF_SIZE as i32 == guard.r {
                // buffer is full.
                if block {
                    // wait for uartstart() to open up space in the buffer.
                    guard.sleep();
                } else {
                    // caller does not want us to wait.
                    return;
                }
            } else {
                let w = guard.w;
                guard.buf[w as usize] = c as u8;
                guard.w = (w + 1) % UART_TX_BUF_SIZE as i32;
                self.start(guard);
                return;
            }
        }
    }

    /// if the UART is idle, and a character is waiting
    /// in the transmit buffer, send it.
    /// caller must hold uart_tx_lock.
    /// called from both the top- and bottom-half.
    fn start(&self, mut guard: SleepablelockGuard<'_, UartTX>) {
        loop {
            if guard.w == guard.r {
                // transmit buffer is empty.
                return;
            }

            if (LSR.read() & (1 << 5)) == 0 {
                // the UART transmit holding register is full,
                // so we cannot give it another byte.
                // it will interrupt when it's ready for a new byte.
                return;
            }

            let r = guard.r;
            let c = guard.buf[r as usize];
            guard.r = (r + 1) % UART_TX_BUF_SIZE as i32;

            // maybe uartputc() is waiting for space in the buffer.
            guard.wakeup();

            THR.write(c);
        }
    }

    /// Read one input character from the UART.
    /// Return -1 if none is waiting.
    /// TODO: should get &self - need to refactor when encapsulate Uart into Console.
    fn getc() -> i32 {
        if LSR.read() & 0x01 != 0 {
            // Input data is ready.
            RBR.read() as i32
        } else {
            -1
        }
    }

    /// handle a uart interrupt, raised because input has
    /// arrived, or the uart is ready for more output, or
    /// both. called from trap.c.
    pub fn intr(&self) {
        // read and process incoming characters.
        loop {
            let c = Uart::getc();
            if c == -1 {
                break;
            }
            unsafe {
                consoleintr(c);
            }
        }

        // send buffered characters.
        self.start(self.tx_lock.lock());
    }
}
