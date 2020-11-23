use crate::{
    file::Devsw,
    kernel::kernel,
    param::NDEV,
    proc::myproc,
    sleepablelock::{Sleepablelock, SleepablelockGuard},
    spinlock::Spinlock,
    uart::Uart,
    utils::spin_loop,
    vm::{UVAddr, VAddr},
};
use core::fmt::{self, Write};

const CONSOLE_IN_DEVSW: usize = 1;
/// Size of console input buffer.
const INPUT_BUF: usize = 128;

/// The integrated interface for console device.
/// Managing every communication between real-world users and system.
pub struct Console {
    /// An interface for user programs.
    /// I/O interactions between users and processes are done by this interface.
    /// Inputs: User commands.
    /// Outputs: Graphic(text) results.
    terminal: Sleepablelock<Terminal>,

    /// A struct for println! macro.
    /// Lock to avoid interleaving concurrent println! macros.
    pub printer: Spinlock<Printer>,

    /// A console's uart interface which guarantees uniqueness.
    /// Interaction with UART Registers should always held by this interface.
    uart: Uart,
}

impl Console {
    pub const fn new() -> Self {
        Self {
            terminal: Sleepablelock::new("Terminal", Terminal::new()),
            printer: Spinlock::new("Println", Printer::new()),
            uart: Uart::new(),
        }
    }

    pub fn init(&mut self, devsw: &mut [Devsw; NDEV]) {
        self.uart.init();

        // Connect read and write system calls
        // to consoleread and consolewrite.
        devsw[CONSOLE_IN_DEVSW] = Devsw {
            read: Some(consoleread),
            write: Some(consolewrite),
        };
    }

    pub fn uartintr(&self) {
        self.uart.intr()
    }

    /// Send one character to the uart.
    pub fn putc(&self, c: i32) {
        // From printf.rs.
        if kernel().is_panicked() {
            spin_loop();
        }
        if c == BACKSPACE {
            // If the user typed backspace, overwrite with a space.
            self.uart.putc_sync('\u{8}' as i32);
            self.uart.putc_sync(' ' as i32);
            self.uart.putc_sync('\u{8}' as i32);
        } else {
            self.uart.putc_sync(c);
        };
    }

    unsafe fn write(&self, src: UVAddr, n: i32) -> i32 {
        let terminal = self.terminal.lock();
        terminal.write(src, n)
    }

    unsafe fn read(&self, dst: UVAddr, n: i32) -> i32 {
        let mut terminal = self.terminal.lock();
        terminal.read(dst, n)
    }

    /// The console input interrupt handler.
    /// uartintr() calls this for input character.
    /// Do erase/kill processing, append to TERMINAL.buf,
    /// wake up consoleread() if a whole line has arrived.
    pub fn intr(&self, cin: i32) {
        let mut terminal = self.terminal.lock();
        terminal.intr(cin);
    }
}

/// Accept user commands and show result texts.
pub struct Terminal {
    buf: [u8; INPUT_BUF],

    /// Read index.
    r: u32,

    /// Write index.
    w: u32,

    /// Edit index.
    e: u32,
}

impl Terminal {
    pub const fn new() -> Self {
        Self {
            buf: [0; INPUT_BUF],
            r: 0,
            w: 0,
            e: 0,
        }
    }
}

impl SleepablelockGuard<'_, Terminal> {
    unsafe fn write(&self, src: UVAddr, n: i32) -> i32 {
        for i in 0..n {
            let mut c = [0 as u8];
            if VAddr::copyin(&mut c, UVAddr::new(src.into_usize() + (i as usize))).is_err() {
                return i;
            }
            kernel().console.uart.putc(c[0] as i32);
        }
        n
    }

    unsafe fn read(&mut self, mut dst: UVAddr, mut n: i32) -> i32 {
        let target = n as u32;
        while n > 0 {
            // Wait until interrupt handler has put some
            // input into CONS.buffer.
            while self.r == self.w {
                if (*myproc()).killed() {
                    return -1;
                }
                self.sleep();
            }
            let fresh0 = self.r;
            self.r = self.r.wrapping_add(1);
            let cin = self.buf[fresh0.wrapping_rem(INPUT_BUF as u32) as usize] as i32;

            // end-of-file
            if cin == ctrl('D') {
                if (n as u32) < target {
                    // Save ^D for next time, to make sure
                    // caller gets a 0-byte result.
                    self.r = self.r.wrapping_sub(1)
                }
                break;
            } else {
                // Copy the input byte to the user-space buffer.
                let cbuf = [cin as u8];
                if UVAddr::copyout(dst, &cbuf).is_err() {
                    break;
                }
                dst = dst + 1;
                n -= 1;
                if cin == '\n' as i32 {
                    // A whole line has arrived, return to
                    // the user-level read().
                    break;
                }
            }
        }
        target.wrapping_sub(n as u32) as i32
    }

    fn intr(&mut self, mut cin: i32) {
        match cin {
            // Print process list.
            m if m == ctrl('P') => unsafe {
                kernel().procs.dump();
            },

            // Kill line.
            m if m == ctrl('U') => {
                while self.e != self.w
                    && self.buf[self.e.wrapping_sub(1).wrapping_rem(INPUT_BUF as u32) as usize]
                        as i32
                        != '\n' as i32
                {
                    self.e = self.e.wrapping_sub(1);
                    kernel().console.uart.putc(BACKSPACE);
                }
            }

            // Backspace
            m if m == ctrl('H') | '\x7f' as i32 => {
                if self.e != self.w {
                    self.e = self.e.wrapping_sub(1);
                    kernel().console.uart.putc(BACKSPACE);
                }
            }

            _ => {
                if cin != 0 && self.e.wrapping_sub(self.r) < INPUT_BUF as u32 {
                    cin = if cin == '\r' as i32 { '\n' as i32 } else { cin };

                    // Echo back to the user.
                    kernel().console.uart.putc(cin);

                    // Store for consumption by consoleread().
                    let fresh1 = self.e;
                    self.e = self.e.wrapping_add(1);
                    self.buf[fresh1.wrapping_rem(INPUT_BUF as u32) as usize] = cin as u8;
                    if cin == '\n' as i32
                        || cin == ctrl('D')
                        || self.e == self.r.wrapping_add(INPUT_BUF as u32)
                    {
                        // Wake up consoleread() if a whole line (or end-of-file)
                        // has arrived.
                        self.w = self.e;
                        self.wakeup();
                    }
                }
            }
        }
    }
}

/// A printer formats UTF-8 bytes.
pub struct Printer {}

impl Printer {
    pub const fn new() -> Self {
        Self {}
    }
}

impl Write for Printer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            kernel().console.putc(c as _);
        }
        Ok(())
    }
}

/// Console input and output, to the uart.
/// Reads are line at a time.
/// Implements special input characters:
///   newline -- end of line
///   control-h -- backspace
///   control-u -- kill line
///   control-d -- end of file
///   control-p -- print process list
const BACKSPACE: i32 = 0x100;

/// Control-x
const fn ctrl(x: char) -> i32 {
    x as i32 - '@' as i32
}

/// User write()s to the console go here.
unsafe fn consolewrite(src: UVAddr, n: i32) -> i32 {
    kernel().console.write(src, n)
}

/// User read()s from the console go here.
/// Copy (up to) a whole input line to dst.
/// User_dist indicates whether dst is a user
/// or kernel address.
unsafe fn consoleread(dst: UVAddr, n: i32) -> i32 {
    kernel().console.read(dst, n)
}
