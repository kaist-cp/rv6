use crate::libc;
use crate::spinlock::{ Spinlock, acquire, initlock, release };
use crate::proc::{ cpu, proc_0 };
use crate::file::devsw;
extern "C" {
    pub type pipe;
    // #[no_mangle]
    // static mut devsw: [devsw; 0];
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut Spinlock);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    #[no_mangle]
    fn either_copyout(
        user_dst: libc::c_int,
        dst: uint64,
        src: *mut libc::c_void,
        len: uint64,
    ) -> libc::c_int;
    #[no_mangle]
    fn either_copyin(
        dst: *mut libc::c_void,
        user_src: libc::c_int,
        src: uint64,
        len: uint64,
    ) -> libc::c_int;
    #[no_mangle]
    fn procdump();
    // spinlock.c
    // #[no_mangle]
    // fn acquire(_: *mut spinlock);
    // #[no_mangle]
    // fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    // #[no_mangle]
    // fn release(_: *mut spinlock);
    // uart.c
    #[no_mangle]
    fn uartinit();
    #[no_mangle]
    fn uartputc(_: libc::c_int);
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
// Mutual exclusion lock.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct spinlock {
//     pub locked: uint,
//     pub name: *mut libc::c_char,
//     pub cpu: *mut cpu,
// }
// Per-CPU state.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct cpu {
//     pub proc_0: *mut proc_0,
//     pub scheduler: context,
//     pub noff: libc::c_int,
//     pub intena: libc::c_int,
// }
// Saved registers for kernel context switches.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct context {
//     pub ra: uint64,
//     pub sp: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
// }
// Per-process state
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct proc_0 {
//     pub lock: Spinlock,
//     pub state: procstate,
//     pub parent: *mut proc_0,
//     pub chan: *mut libc::c_void,
//     pub killed: libc::c_int,
//     pub xstate: libc::c_int,
//     pub pid: libc::c_int,
//     pub kstack: uint64,
//     pub sz: uint64,
//     pub pagetable: pagetable_t,
//     pub tf: *mut trapframe,
//     pub context: context,
//     pub ofile: [*mut file; 16],
//     pub cwd: *mut inode,
//     pub name: [libc::c_char; 16],
// }
// FD_DEVICE
// in-memory copy of an inode
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct inode {
//     pub dev: uint,
//     pub inum: uint,
//     pub ref_0: libc::c_int,
//     pub lock: sleeplock,
//     pub valid: libc::c_int,
//     pub type_0: libc::c_short,
//     pub major: libc::c_short,
//     pub minor: libc::c_short,
//     pub nlink: libc::c_short,
//     pub size: uint,
//     pub addrs: [uint; 13],
// }
// Long-term locks for processes
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct sleeplock {
//     pub locked: uint,
//     pub lk: Spinlock,
//     pub name: *mut libc::c_char,
//     pub pid: libc::c_int,
// }
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct file {
//     pub type_0: C2RustUnnamed,
//     pub ref_0: libc::c_int,
//     pub readable: libc::c_char,
//     pub writable: libc::c_char,
//     pub pipe: *mut pipe,
//     pub ip: *mut inode,
//     pub off: uint,
//     pub major: libc::c_short,
// }
pub type C2RustUnnamed = libc::c_uint;
// pub const FD_DEVICE: C2RustUnnamed = 3;
// pub const FD_INODE: C2RustUnnamed = 2;
// pub const FD_PIPE: C2RustUnnamed = 1;
// pub const FD_NONE: C2RustUnnamed = 0;
// per-process data for the trap handling code in trampoline.S.
// sits in a page by itself just under the trampoline page in the
// user page table. not specially mapped in the kernel page table.
// the sscratch register points here.
// uservec in trampoline.S saves user registers in the trapframe,
// then initializes registers from the trapframe's
// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// usertrapret() and userret in trampoline.S set up
// the trapframe's kernel_*, restore user registers from the
// trapframe, switch to the user page table, and enter user space.
// the trapframe includes callee-saved user registers like s0-s11 because the
// return-to-user path via usertrapret() doesn't return through
// the entire kernel call stack.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct trapframe {
//     pub kernel_satp: uint64,
//     pub kernel_sp: uint64,
//     pub kernel_trap: uint64,
//     pub epc: uint64,
//     pub kernel_hartid: uint64,
//     pub ra: uint64,
//     pub sp: uint64,
//     pub gp: uint64,
//     pub tp: uint64,
//     pub t0: uint64,
//     pub t1: uint64,
//     pub t2: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub a0: uint64,
//     pub a1: uint64,
//     pub a2: uint64,
//     pub a3: uint64,
//     pub a4: uint64,
//     pub a5: uint64,
//     pub a6: uint64,
//     pub a7: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
//     pub t3: uint64,
//     pub t4: uint64,
//     pub t5: uint64,
//     pub t6: uint64,
// }
// pub type pagetable_t = *mut uint64;
// pub type procstate = libc::c_uint;
// pub const ZOMBIE: procstate = 4;
// pub const RUNNING: procstate = 3;
// pub const RUNNABLE: procstate = 2;
// pub const SLEEPING: procstate = 1;
// pub const UNUSED: procstate = 0;
// map major device number to device functions.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct devsw {
//     pub read:
//         Option<unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int>,
//     pub write:
//         Option<unsafe extern "C" fn(_: libc::c_int, _: uint64, _: libc::c_int) -> libc::c_int>,
// }
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
// Control-x
//
// send one character to the uart.
//
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
//
// user write()s to the console go here.
//
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
//
// user read()s from the console go here.
// copy (up to) a whole input line to dst.
// user_dist indicates whether dst is a user
// or kernel address.
//
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
//
// the console input interrupt handler.
// uartintr() calls this for input character.
// do erase/kill processing, append to cons.buf,
// wake up consoleread() if a whole line has arrived.
//
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
// console.c
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
