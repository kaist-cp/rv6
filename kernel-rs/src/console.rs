use crate::file::devsw;
use crate::libc;
use crate::proc::{cpu, either_copyin, either_copyout, myproc, procdump, sleep, wakeup};
use crate::spinlock::{acquire, initlock, release, Spinlock};
use crate::uart::{uartinit, uartputc};
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type C2RustUnnamed = libc::c_uint;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub lock: Spinlock,
    pub buf: [libc::c_char; 128],
    pub r: uint,
    pub w: uint,
    pub e: uint,
}
pub const CONSOLE: libc::c_int = 1 as libc::c_int;
//
// Console input and output, to the uart.
// Reads are line at a time.
// Implements special input characters:
//   newline -- end of line
//   control-h -- backspace
//   control-u -- kill line
//   control-d -- end of file
//   control-p -- print process list
//
pub const BACKSPACE: libc::c_int = 0x100 as libc::c_int;
/// Control-x
///
/// send one character to the uart.
///
#[no_mangle]
pub unsafe extern "C" fn consputc(mut c: libc::c_int) {
    extern "C" {
        #[no_mangle]
        static mut panicked: libc::c_int;
    } // from printf.c
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
pub const INPUT_BUF: libc::c_int = 128 as libc::c_int;
// Edit index
#[no_mangle]
pub static mut cons: C2RustUnnamed_0 = C2RustUnnamed_0 {
    lock: Spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    buf: [0; 128],
    r: 0,
    w: 0,
    e: 0,
};
///
/// user write()s to the console go here.
///
#[no_mangle]
pub unsafe extern "C" fn consolewrite(
    mut user_src: libc::c_int,
    mut src: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut i: libc::c_int = 0;
    acquire(&mut cons.lock);
    i = 0 as libc::c_int;
    while i < n {
        let mut c: libc::c_char = 0;
        if either_copyin(
            &mut c as *mut libc::c_char as *mut libc::c_void,
            user_src,
            src.wrapping_add(i as libc::c_ulong),
            1 as libc::c_int as uint64,
        ) == -(1 as libc::c_int)
        {
            break;
        }
        consputc(c as libc::c_int);
        i += 1
    }
    release(&mut cons.lock);
    n
}
///
/// user read()s from the console go here.
/// copy (up to) a whole input line to dst.
/// user_dist indicates whether dst is a user
/// or kernel address.
///
#[no_mangle]
pub unsafe extern "C" fn consoleread(
    mut user_dst: libc::c_int,
    mut dst: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut target: uint = 0;
    let mut c: libc::c_int = 0;
    let mut cbuf: libc::c_char = 0;
    target = n as uint;
    acquire(&mut cons.lock);
    while n > 0 as libc::c_int {
        // wait until interrupt handler has put some
        // input into cons.buffer.
        while cons.r == cons.w {
            if (*myproc()).killed != 0 {
                release(&mut cons.lock);
                return -(1 as libc::c_int);
            }
            sleep(
                &mut cons.r as *mut uint as *mut libc::c_void,
                &mut cons.lock,
            );
        }
        let fresh0 = cons.r;
        cons.r = cons.r.wrapping_add(1);
        c = cons.buf[fresh0.wrapping_rem(INPUT_BUF as libc::c_uint) as usize] as libc::c_int;
        if c == 'D' as i32 - '@' as i32 {
            // end-of-file
            if (n as libc::c_uint) < target {
                // Save ^D for next time, to make sure
                // caller gets a 0-byte result.
                cons.r = cons.r.wrapping_sub(1)
            }
            break;
        } else {
            // copy the input byte to the user-space buffer.
            cbuf = c as libc::c_char;
            if either_copyout(
                user_dst,
                dst,
                &mut cbuf as *mut libc::c_char as *mut libc::c_void,
                1 as libc::c_int as uint64,
            ) == -(1 as libc::c_int)
            {
                break;
            }
            dst = dst.wrapping_add(1);
            n -= 1;
            if c == '\n' as i32 {
                break;
            }
        }
    }
    release(&mut cons.lock);
    target.wrapping_sub(n as libc::c_uint) as libc::c_int
}
///
/// the console input interrupt handler.
/// uartintr() calls this for input character.
/// do erase/kill processing, append to cons.buf,
/// wake up consoleread() if a whole line has arrived.
///
#[no_mangle]
pub unsafe extern "C" fn consoleintr(mut c: libc::c_int) {
    acquire(&mut cons.lock);
    match c {
        16 => {
            // Print process list.
            procdump();
        }
        21 => {
            // Kill line.
            while cons.e != cons.w
                && cons.buf[cons
                    .e
                    .wrapping_sub(1 as libc::c_int as libc::c_uint)
                    .wrapping_rem(INPUT_BUF as libc::c_uint) as usize]
                    as libc::c_int
                    != '\n' as i32
            {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        8 | 127 => {
            // Backspace
            if cons.e != cons.w {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        _ => {
            if c != 0 as libc::c_int && cons.e.wrapping_sub(cons.r) < INPUT_BUF as libc::c_uint {
                c = if c == '\r' as i32 { '\n' as i32 } else { c };
                // echo back to the user.
                consputc(c);
                // store for consumption by consoleread().
                let fresh1 = cons.e;
                cons.e = cons.e.wrapping_add(1);
                cons.buf[fresh1.wrapping_rem(INPUT_BUF as libc::c_uint) as usize] =
                    c as libc::c_char;
                if c == '\n' as i32
                    || c == 'D' as i32 - '@' as i32
                    || cons.e == cons.r.wrapping_add(INPUT_BUF as libc::c_uint)
                {
                    // wake up consoleread() if a whole line (or end-of-file)
                    // has arrived.
                    cons.w = cons.e;
                    wakeup(&mut cons.r as *mut uint as *mut libc::c_void);
                }
            }
        }
    }
    release(&mut cons.lock);
}
#[no_mangle]
pub unsafe extern "C" fn consoleinit() {
    initlock(
        &mut cons.lock,
        b"cons\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    uartinit();
    // connect read and write system calls
    // to consoleread and consolewrite.
    let fresh2 = &mut (*devsw.as_mut_ptr().offset(CONSOLE as isize)).read;
    *fresh2 = Some(
        consoleread
            as unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int,
    );
    let fresh3 = &mut (*devsw.as_mut_ptr().offset(CONSOLE as isize)).write;
    *fresh3 = Some(
        consolewrite
            as unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int,
    );
}
