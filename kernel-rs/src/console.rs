use crate::libc;
use crate::{
    file::{CONSOLE, DEVSW},
    printf::PANICKED,
    proc::{either_copyin, either_copyout, myproc, procdump, sleep, wakeup},
    spinlock::RawSpinlock,
    uart::{uartinit, uartputc},
};

/// input
const INPUT_BUF: usize = 128;

struct Console {
    lock: RawSpinlock,
    buf: [u8; 128],

    /// Read index
    r: u32,

    /// Write index
    w: u32,

    /// Edit index
    e: u32,
}

impl Console {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            buf: [0; INPUT_BUF],
            r: 0,
            w: 0,
            e: 0,
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

/// send one character to the uart.
pub unsafe fn consputc(c: i32) {
    // from printf.rs
    if PANICKED != 0 {
        loop {}
    }
    if c == BACKSPACE {
        // if the user typed backspace, overwrite with a space.
        uartputc('\u{8}' as i32);
        uartputc(' ' as i32);
        uartputc('\u{8}' as i32);
    } else {
        uartputc(c);
    };
}

static mut CONS: Console = Console::zeroed();

/// user write()s to the console go here.
unsafe fn consolewrite(user_src: i32, src: usize, n: i32) -> i32 {
    CONS.lock.acquire();
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
        consputc(c as i32);
    }
    CONS.lock.release();
    n
}

/// user read()s from the console go here.
/// copy (up to) a whole input line to dst.
/// user_dist indicates whether dst is a user
/// or kernel address.
unsafe fn consoleread(user_dst: i32, mut dst: usize, mut n: i32) -> i32 {
    let target: u32 = n as u32;
    CONS.lock.acquire();
    while n > 0 {
        // wait until interrupt handler has put some
        // input into CONS.buffer.
        while CONS.r == CONS.w {
            if (*myproc()).killed != 0 {
                CONS.lock.release();
                return -1;
            }
            sleep(&mut CONS.r as *mut u32 as *mut libc::CVoid, &mut CONS.lock);
        }
        let fresh0 = CONS.r;
        CONS.r = CONS.r.wrapping_add(1);
        let cin = CONS.buf[fresh0.wrapping_rem(INPUT_BUF as u32) as usize] as i32;

        // end-of-file
        if cin == ctrl('D') {
            if (n as u32) < target {
                // Save ^D for next time, to make sure
                // caller gets a 0-byte result.
                CONS.r = CONS.r.wrapping_sub(1)
            }
            break;
        } else {
            // copy the input byte to the user-space buffer.
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
                // a whole line has arrived, return to
                // the user-level read().
                break;
            }
        }
    }
    CONS.lock.release();
    target.wrapping_sub(n as u32) as i32
}

/// the console input interrupt handler.
/// uartintr() calls this for input character.
/// do erase/kill processing, append to CONS.buf,
/// wake up consoleread() if a whole line has arrived.
pub unsafe fn consoleintr(mut cin: i32) {
    CONS.lock.acquire();
    match cin {
        // Print process list.
        m if m == ctrl('P') => {
            procdump();
        }

        // Kill line.
        m if m == ctrl('U') => {
            while CONS.e != CONS.w
                && CONS.buf[CONS.e.wrapping_sub(1).wrapping_rem(INPUT_BUF as u32) as usize] as i32
                    != '\n' as i32
            {
                CONS.e = CONS.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }

        // Backspace
        m if m == ctrl('H') | '\x7f' as i32 => {
            if CONS.e != CONS.w {
                CONS.e = CONS.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        _ => {
            if cin != 0 && CONS.e.wrapping_sub(CONS.r) < INPUT_BUF as u32 {
                cin = if cin == '\r' as i32 { '\n' as i32 } else { cin };

                // echo back to the user.
                consputc(cin);

                // store for consumption by consoleread().
                let fresh1 = CONS.e;
                CONS.e = CONS.e.wrapping_add(1);
                CONS.buf[fresh1.wrapping_rem(INPUT_BUF as u32) as usize] = cin as u8;
                if cin == '\n' as i32
                    || cin == ctrl('D')
                    || CONS.e == CONS.r.wrapping_add(INPUT_BUF as u32)
                {
                    // wake up consoleread() if a whole line (or end-of-file)
                    // has arrived.
                    CONS.w = CONS.e;
                    wakeup(&mut CONS.r as *mut u32 as *mut libc::CVoid);
                }
            }
        }
    }
    CONS.lock.release();
}

pub unsafe fn consoleinit() {
    CONS.lock.initlock(b"CONS\x00" as *const u8 as *mut u8);
    uartinit();

    // connect read and write system calls
    // to consoleread and consolewrite.
    let fresh2 = &mut (*DEVSW.as_mut_ptr().offset(CONSOLE)).read;
    *fresh2 = Some(consoleread as unsafe fn(_: i32, _: usize, _: i32) -> i32);
    let fresh3 = &mut (*DEVSW.as_mut_ptr().offset(CONSOLE)).write;
    *fresh3 = Some(consolewrite as unsafe fn(_: i32, _: usize, _: i32) -> i32);
}
