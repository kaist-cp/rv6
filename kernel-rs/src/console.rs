use core::fmt;

use crate::{
    file::Devsw,
    kernel::{kernel, kernel_builder},
    lock::SleepablelockGuard,
    param::NDEV,
    uart::Uart,
    vm::UVAddr,
};

const CONSOLE_IN_DEVSW: usize = 1;
/// Size of console input buffer.
const INPUT_BUF: usize = 128;

pub struct Console {
    buf: [u8; INPUT_BUF],

    /// Read index.
    r: u32,

    /// Write index.
    w: u32,

    /// Edit index.
    e: u32,
}

impl Console {
    pub const fn new() -> Self {
        Self {
            buf: [0; INPUT_BUF],
            r: 0,
            w: 0,
            e: 0,
        }
    }

    /// putc for Console.
    /// TODO(https://github.com/kaist-cp/rv6/issues/298)
    /// This function should be changed after refactoring Console-Uart-Printer relationship.
    pub fn putc(&mut self, c: i32) {
        putc(c);
    }

    unsafe fn write(&mut self, src: UVAddr, n: i32) -> i32 {
        for i in 0..n {
            let mut c = [0u8];
            // TODO: remove kernel_builder()
            if kernel_builder()
                .current_proc()
                .expect("No current proc")
                .memory_mut()
                .copy_in_bytes(&mut c, src + i as usize)
                .is_err()
            {
                return i;
            }
            // TODO(https://github.com/kaist-cp/rv6/issues/298): Temporarily using global function kernel().
            // This implementation should be changed after refactoring Console-Uart-Printer relationship.
            kernel_builder().uart.putc(c[0] as i32);
        }
        n
    }

    unsafe fn read(this: &mut SleepablelockGuard<'_, Self>, mut dst: UVAddr, mut n: i32) -> i32 {
        let target = n as u32;
        while n > 0 {
            // Wait until interrupt handler has put some
            // input into CONS.buffer.
            while this.r == this.w {
                // TODO: remove kernel_builder()
                if kernel_builder()
                    .current_proc()
                    .expect("No current proc")
                    .killed()
                {
                    return -1;
                }
                this.sleep();
            }
            let fresh0 = this.r;
            this.r = this.r.wrapping_add(1);
            let cin = this.buf[fresh0.wrapping_rem(INPUT_BUF as u32) as usize] as i32;

            // end-of-file
            if cin == ctrl('D') {
                if (n as u32) < target {
                    // Save ^D for next time, to make sure
                    // caller gets a 0-byte result.
                    this.r = this.r.wrapping_sub(1)
                }
                break;
            } else {
                // Copy the input byte to the user-space buffer.
                let cbuf = [cin as u8];
                // TODO: remove kernel_builder()
                if kernel_builder()
                    .current_proc()
                    .expect("No current proc")
                    .memory_mut()
                    .copy_out_bytes(dst, &cbuf)
                    .is_err()
                {
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

    unsafe fn intr(this: &mut SleepablelockGuard<'_, Self>, mut cin: i32) {
        match cin {
            // Print process list.
            m if m == ctrl('P') => {
                // TODO: remove kernel()
                unsafe { kernel().procs().dump() };
            }

            // Kill line.
            m if m == ctrl('U') => {
                while this.e != this.w
                    && this.buf[this.e.wrapping_sub(1).wrapping_rem(INPUT_BUF as u32) as usize]
                        as i32
                        != '\n' as i32
                {
                    this.e = this.e.wrapping_sub(1);
                    this.putc(BACKSPACE);
                }
            }

            // Backspace
            m if m == ctrl('H') | '\x7f' as i32 => {
                if this.e != this.w {
                    this.e = this.e.wrapping_sub(1);
                    this.putc(BACKSPACE);
                }
            }
            _ => {
                if cin != 0 && this.e.wrapping_sub(this.r) < INPUT_BUF as u32 {
                    cin = if cin == '\r' as i32 { '\n' as i32 } else { cin };

                    // Echo back to the user.
                    this.putc(cin);

                    // Store for consumption by consoleread().
                    let fresh1 = this.e;
                    this.e = this.e.wrapping_add(1);
                    this.buf[fresh1.wrapping_rem(INPUT_BUF as u32) as usize] = cin as u8;
                    if cin == '\n' as i32
                        || cin == ctrl('D')
                        || this.e == this.r.wrapping_add(INPUT_BUF as u32)
                    {
                        // Wake up consoleread() if a whole line (or end-of-file)
                        // has arrived.
                        this.w = this.e;
                        this.wakeup();
                    }
                }
            }
        }
    }
}

pub struct Printer {}

impl Printer {
    pub const fn new() -> Self {
        Self {}
    }

    /// putc for Printer.
    /// TODO(https://github.com/kaist-cp/rv6/issues/298)
    /// This function should be changed after refactoring Console-Uart-Printer relationship.
    pub fn putc(&mut self, c: i32) {
        putc(c);
    }
}

impl fmt::Write for Printer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            self.putc(c as _);
        }
        Ok(())
    }
}

/// Send one character to the uart.
/// TODO(https://github.com/kaist-cp/rv6/issues/298): This global function is temporary.
/// After refactoring Console-Uart-Printer relationship, this function need to be removed.
pub fn putc(c: i32) {
    if c == BACKSPACE {
        // If the user typed backspace, overwrite with a space.
        Uart::putc_sync('\u{8}' as i32);
        Uart::putc_sync(' ' as i32);
        Uart::putc_sync('\u{8}' as i32);
    } else {
        Uart::putc_sync(c);
    };
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

pub unsafe fn consoleinit(devsw: &mut [Devsw; NDEV]) {
    // Connect read and write system calls
    // to consoleread and consolewrite.
    devsw[CONSOLE_IN_DEVSW] = Devsw {
        read: Some(consoleread),
        write: Some(consolewrite),
    };
}

/// User write()s to the console go here.
fn consolewrite(src: UVAddr, n: i32) -> i32 {
    // TODO(https://github.com/kaist-cp/rv6/issues/298) Remove below comment.
    // consolewrite() does not need console.lock() -- can lead to sleep() with lock held.
    // TODO: remove kernel_builder()
    unsafe { (*kernel_builder().console.get_mut_raw()).write(src, n) }
}

/// User read()s from the console go here.
/// Copy (up to) a whole input line to dst.
/// User_dist indicates whether dst is a user
/// or kernel address.
fn consoleread(dst: UVAddr, n: i32) -> i32 {
    // TODO: remove kernel_builder()
    let mut console = kernel_builder().console.lock();
    unsafe { Console::read(&mut console, dst, n) }
}

/// The console input interrupt handler.
/// uartintr() calls this for input character.
/// Do erase/kill processing, append to CONS.buf,
/// wake up consoleread() if a whole line has arrived.
pub unsafe fn consoleintr(cin: i32) {
    // TODO: remove kernel_builder()
    let mut console = kernel_builder().console.lock();
    unsafe { Console::intr(&mut console, cin) };
}
