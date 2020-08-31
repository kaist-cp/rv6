use crate::libc;
use crate::{
    file::{CONSOLE, DEVSW},
    printf::PANICKED,
    proc::{either_copyin, either_copyout, myproc, procdump, sleep, wakeup},
    spinlock::{RawSpinlock, Spinlock},
    uart::Uart,
    utils::spin_loop,
};
use core::fmt;
use core::sync::atomic::Ordering;

/// Size of console input buffer.
const INPUT_BUF: usize = 128;

pub struct Console {
    buf: [u8; INPUT_BUF],

    /// Read index
    r: u32,

    /// Write index
    w: u32,

    /// Edit index
    e: u32,

    uart: Uart,
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            self.putc(c as _);
        }
        Ok(())
    }
}

impl Console {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            buf: [0; INPUT_BUF],
            r: 0,
            w: 0,
            e: 0,
            uart: Uart::zeroed(),
        }
    }

    pub unsafe fn consoleinit() {
        Uart::new();

        // Connect read and write system calls
        // to consoleread and consolewrite.
        let fresh2 = &mut (*DEVSW.as_mut_ptr().add(CONSOLE)).read;
        *fresh2 = Some(consoleread as unsafe fn(_: i32, _: usize, _: i32) -> i32);
        let fresh3 = &mut (*DEVSW.as_mut_ptr().add(CONSOLE)).write;
        *fresh3 = Some(consolewrite as unsafe fn(_: i32, _: usize, _: i32) -> i32);
    }

    /// Send one character to the uart.
    pub fn putc(&mut self, c: i32) {
        // From printf.rs.
        if PANICKED.load(Ordering::Acquire) {
            spin_loop();
        }
        if c == BACKSPACE {
            // If the user typed backspace, overwrite with a space.
            self.uart.putc('\u{8}' as i32);
            self.uart.putc(' ' as i32);
            self.uart.putc('\u{8}' as i32);
        } else {
            self.uart.putc(c);
        };
    }

    unsafe fn write(&mut self, user_src: i32, src: usize, n: i32) {
        for i in 0..n {
            let mut c: u8 = 0;
            if either_copyin(
                &mut c as *mut u8 as *mut libc::CVoid,
                user_src,
                src.wrapping_add(i as usize),
                1usize,
            ) == -1
            {
                break;
            }
            self.putc(c as i32);
        }
    }

    unsafe fn read(
        &mut self,
        user_dst: i32,
        mut dst: usize,
        mut n: i32,
        lk: *mut RawSpinlock,
    ) -> i32 {
        let target = n as u32;
        while n > 0 {
            // Wait until interrupt handler has put some
            // input into CONS.buffer.
            while self.r == self.w {
                if (*myproc()).killed != 0 {
                    return -1;
                }
                // TODO: need to change "RawSpinlock" after refactoring "sleep()" function in proc.rs
                sleep(&mut self.r as *mut u32 as *mut libc::CVoid, lk);
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
                let mut cbuf = cin as u8;
                if either_copyout(
                    user_dst,
                    dst,
                    &mut cbuf as *mut u8 as *mut libc::CVoid,
                    1usize,
                ) == -1
                {
                    break;
                }
                dst = dst.wrapping_add(1);
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

    unsafe fn intr(&mut self, mut cin: i32) {
        match cin {
            // Print process list.
            m if m == ctrl('P') => {
                procdump();
            }

            // Kill line.
            m if m == ctrl('U') => {
                while self.e != self.w
                    && self.buf[self.e.wrapping_sub(1).wrapping_rem(INPUT_BUF as u32) as usize]
                        as i32
                        != '\n' as i32
                {
                    self.e = self.e.wrapping_sub(1);
                    self.putc(BACKSPACE);
                }
            }

            // Backspace
            m if m == ctrl('H') | '\x7f' as i32 => {
                if self.e != self.w {
                    self.e = self.e.wrapping_sub(1);
                    self.putc(BACKSPACE);
                }
            }
            _ => {
                if cin != 0 && self.e.wrapping_sub(self.r) < INPUT_BUF as u32 {
                    cin = if cin == '\r' as i32 { '\n' as i32 } else { cin };

                    // Echo back to the user.
                    self.putc(cin);

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
                        wakeup(&mut self.r as *mut u32 as *mut libc::CVoid);
                    }
                }
            }
        }
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

pub static CONS: Spinlock<Console> = Spinlock::new("CONS", Console::zeroed());

/// User write()s to the console go here.
unsafe fn consolewrite(user_src: i32, src: usize, n: i32) -> i32 {
    let mut console = CONS.lock();
    console.write(user_src, src, n);
    n
}

/// User read()s from the console go here.
/// Copy (up to) a whole input line to dst.
/// User_dist indicates whether dst is a user
/// or kernel address.
unsafe fn consoleread(user_dst: i32, dst: usize, n: i32) -> i32 {
    let mut console = CONS.lock();
    let lk = console.raw() as *mut RawSpinlock;
    console.read(user_dst, dst, n, lk)
}

/// The console input interrupt handler.
/// uartintr() calls this for input character.
/// Do erase/kill processing, append to CONS.buf,
/// wake up consoleread() if a whole line has arrived.
pub unsafe fn consoleintr(cin: i32) {
    let mut console = CONS.lock();
    console.intr(cin);
}
