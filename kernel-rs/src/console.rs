use crate::libc;
use crate::{
    file::{devsw, CONSOLE},
    printf::panicked,
    proc::{either_copyin, either_copyout, myproc, procdump, sleep, wakeup},
    spinlock::{acquire, initlock, release, Spinlock},
    uart::{uartinit, uartputc},
};

#[derive(Copy, Clone)]
pub struct Console {
    pub lock: Spinlock,
    pub buf: [libc::c_char; 128],

    /// Read index
    pub r: u32,

    /// Write index
    pub w: u32,

    /// Edit index
    pub e: u32,
}

impl Console {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
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
pub const BACKSPACE: i32 = 0x100;

/// Control-x
const fn ctrl(x: char) -> i32 {
    x as i32 - '@' as i32
}

/// send one character to the uart.
pub unsafe fn consputc(mut c: i32) {
    if panicked != 0 {
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

/// input
pub const INPUT_BUF: usize = 128;

pub static mut cons: Console = Console::zeroed();

/// user write()s to the console go here.
pub unsafe fn consolewrite(mut user_src: i32, mut src: u64, mut n: i32) -> i32 {
    let mut i: i32 = 0;
    acquire(&mut cons.lock);
    while i < n {
        let mut c: libc::c_char = 0;
        if either_copyin(
            &mut c as *mut libc::c_char as *mut libc::c_void,
            user_src,
            src.wrapping_add(i as u64),
            1 as i32 as u64,
        ) == -(1 as i32)
        {
            break;
        }
        consputc(c as i32);
        i += 1
    }
    release(&mut cons.lock);
    n
}

/// user read()s from the console go here.
/// copy (up to) a whole input line to dst.
/// user_dist indicates whether dst is a user
/// or kernel address.
pub unsafe fn consoleread(mut user_dst: i32, mut dst: u64, mut n: i32) -> i32 {
    let mut target: u32 = n as u32;
    acquire(&mut cons.lock);
    while n > 0 as i32 {
        // wait until interrupt handler has put some
        // input into cons.buffer.
        while cons.r == cons.w {
            if (*myproc()).killed != 0 {
                release(&mut cons.lock);
                return -(1 as i32);
            }
            sleep(&mut cons.r as *mut u32 as *mut libc::c_void, &mut cons.lock);
        }
        let fresh0 = cons.r;
        cons.r = cons.r.wrapping_add(1);
        let cin: i32 = cons.buf[fresh0.wrapping_rem(INPUT_BUF as u32) as usize] as i32;

        // end-of-file
        if cin == ctrl('D') {
            if (n as u32) < target {
                // Save ^D for next time, to make sure
                // caller gets a 0-byte result.
                cons.r = cons.r.wrapping_sub(1)
            }
            break;
        } else {
            // copy the input byte to the user-space buffer.
            let mut cbuf = cin as libc::c_char;
            if either_copyout(
                user_dst,
                dst,
                &mut cbuf as *mut libc::c_char as *mut libc::c_void,
                1 as i32 as u64,
            ) == -(1 as i32)
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
    release(&mut cons.lock);
    target.wrapping_sub(n as u32) as i32
}

/// the console input interrupt handler.
/// uartintr() calls this for input character.
/// do erase/kill processing, append to cons.buf,
/// wake up consoleread() if a whole line has arrived.
pub unsafe fn consoleintr(mut cin: i32) {
    acquire(&mut cons.lock);
    match cin {
        // Print process list.
        m if m == ctrl('P') => {
            procdump();
        }

        // Kill line.
        m if m == ctrl('U') => {
            while cons.e != cons.w
                && cons.buf[cons
                    .e
                    .wrapping_sub(1 as i32 as u32)
                    .wrapping_rem(INPUT_BUF as u32) as usize] as i32
                    != '\n' as i32
            {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }

        // Backspace
        m if m == ctrl('H') | '\x7f' as i32 => {
            if cons.e != cons.w {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        _ => {
            if cin != 0 as i32 && cons.e.wrapping_sub(cons.r) < INPUT_BUF as u32 {
                cin = if cin == '\r' as i32 { '\n' as i32 } else { cin };

                // echo back to the user.
                consputc(cin);

                // store for consumption by consoleread().
                let fresh1 = cons.e;
                cons.e = cons.e.wrapping_add(1);
                cons.buf[fresh1.wrapping_rem(INPUT_BUF as u32) as usize] = cin as libc::c_char;
                if cin == '\n' as i32
                    || cin == ctrl('D')
                    || cons.e == cons.r.wrapping_add(INPUT_BUF as u32)
                {
                    // wake up consoleread() if a whole line (or end-of-file)
                    // has arrived.
                    cons.w = cons.e;
                    wakeup(&mut cons.r as *mut u32 as *mut libc::c_void);
                }
            }
        }
    }
    release(&mut cons.lock);
}

pub unsafe fn consoleinit() {
    initlock(
        &mut cons.lock,
        b"cons\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    uartinit();

    // connect read and write system calls
    // to consoleread and consolewrite.
    let fresh2 = &mut (*devsw.as_mut_ptr().offset(CONSOLE)).read;
    *fresh2 = Some(consoleread as unsafe fn(_: i32, _: u64, _: i32) -> i32);
    let fresh3 = &mut (*devsw.as_mut_ptr().offset(CONSOLE)).write;
    *fresh3 = Some(consolewrite as unsafe fn(_: i32, _: u64, _: i32) -> i32);
}
