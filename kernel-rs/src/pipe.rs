use crate::libc;
use crate::{
    file::{filealloc, fileclose, File},
    kalloc::{kalloc, kfree},
    proc::{myproc, proc_0, sleep, wakeup},
    spinlock::{acquire, initlock, release, Spinlock},
    vm::{copyin, copyout},
};
use core::ptr;
#[derive(Copy, Clone)]
pub struct Pipe {
    pub lock: Spinlock,
    pub data: [libc::c_char; PIPESIZE as usize],
    /// number of bytes read
    pub nread: u32,
    /// number of bytes written
    pub nwrite: u32,
    /// read fd is still open
    pub readopen: i32,
    /// write fd is still open
    pub writeopen: i32,
}
pub const FD_DEVICE: u32 = 3;
pub const FD_INODE: u32 = 2;
pub const FD_PIPE: u32 = 1;
pub const FD_NONE: u32 = 0;
pub const PIPESIZE: i32 = 512;
/// write fd is still open
pub unsafe fn pipealloc(mut f0: *mut *mut File, mut f1: *mut *mut File) -> i32 {
    let mut pi: *mut Pipe = ptr::null_mut();
    pi = ptr::null_mut();
    *f1 = 0 as *mut File;
    *f0 = *f1;
    *f0 = filealloc();
    if !((*f0).is_null() || {
        *f1 = filealloc();
        (*f1).is_null()
    }) {
        pi = kalloc() as *mut Pipe;
        if !pi.is_null() {
            (*pi).readopen = 1 as i32;
            (*pi).writeopen = 1 as i32;
            (*pi).nwrite = 0 as u32;
            (*pi).nread = 0 as u32;
            initlock(
                &mut (*pi).lock,
                b"pipe\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
            (**f0).typ = FD_PIPE;
            (**f0).readable = 1 as libc::c_char;
            (**f0).writable = 0 as libc::c_char;
            (**f0).pipe = pi;
            (**f1).typ = FD_PIPE;
            (**f1).readable = 0 as libc::c_char;
            (**f1).writable = 1 as libc::c_char;
            (**f1).pipe = pi;
            return 0;
        }
    }
    if !pi.is_null() {
        kfree(pi as *mut libc::c_char as *mut libc::c_void);
    }
    if !(*f0).is_null() {
        fileclose(*f0);
    }
    if !(*f1).is_null() {
        fileclose(*f1);
    }
    -(1 as i32)
}
pub unsafe fn pipeclose(mut pi: *mut Pipe, mut writable: i32) {
    acquire(&mut (*pi).lock);
    if writable != 0 {
        (*pi).writeopen = 0 as i32;
        wakeup(&mut (*pi).nread as *mut u32 as *mut libc::c_void);
    } else {
        (*pi).readopen = 0 as i32;
        wakeup(&mut (*pi).nwrite as *mut u32 as *mut libc::c_void);
    }
    if (*pi).readopen == 0 as i32 && (*pi).writeopen == 0 as i32 {
        release(&mut (*pi).lock);
        kfree(pi as *mut libc::c_char as *mut libc::c_void);
    } else {
        release(&mut (*pi).lock);
    };
}
pub unsafe fn pipewrite(mut pi: *mut Pipe, mut addr: u64, mut n: i32) -> i32 {
    let mut i: i32 = 0;
    let mut ch: libc::c_char = 0;
    let mut pr: *mut proc_0 = myproc();
    acquire(&mut (*pi).lock);
    while i < n {
        while (*pi).nwrite == (*pi).nread.wrapping_add(PIPESIZE as u32) {
            //DOC: pipewrite-full
            if (*pi).readopen == 0 as i32 || (*myproc()).killed != 0 {
                release(&mut (*pi).lock);
                return -(1 as i32);
            }
            wakeup(&mut (*pi).nread as *mut u32 as *mut libc::c_void);
            sleep(
                &mut (*pi).nwrite as *mut u32 as *mut libc::c_void,
                &mut (*pi).lock,
            );
        }
        if copyin(
            (*pr).pagetable,
            &mut ch,
            addr.wrapping_add(i as u64),
            1 as i32 as u64,
        ) == -(1 as i32)
        {
            break;
        }
        let fresh0 = (*pi).nwrite;
        (*pi).nwrite = (*pi).nwrite.wrapping_add(1);
        (*pi).data[fresh0.wrapping_rem(PIPESIZE as u32) as usize] = ch;
        i += 1
    }
    wakeup(&mut (*pi).nread as *mut u32 as *mut libc::c_void);
    release(&mut (*pi).lock);
    n
}
pub unsafe fn piperead(mut pi: *mut Pipe, mut addr: u64, mut n: i32) -> i32 {
    let mut i: i32 = 0;
    let mut pr: *mut proc_0 = myproc();
    let mut ch: libc::c_char = 0;
    acquire(&mut (*pi).lock);
    while (*pi).nread == (*pi).nwrite && (*pi).writeopen != 0 {
        //DOC: pipe-empty
        if (*myproc()).killed != 0 {
            release(&mut (*pi).lock);
            return -(1 as i32);
        }
        sleep(
            &mut (*pi).nread as *mut u32 as *mut libc::c_void,
            &mut (*pi).lock,
        );
        //DOC: piperead-sleep
    }
    while i < n {
        //DOC: piperead-copy
        if (*pi).nread == (*pi).nwrite {
            break; //DOC: piperead-wakeup
        }
        let fresh1 = (*pi).nread;
        (*pi).nread = (*pi).nread.wrapping_add(1);
        ch = (*pi).data[fresh1.wrapping_rem(PIPESIZE as u32) as usize];
        if copyout(
            (*pr).pagetable,
            addr.wrapping_add(i as u64),
            &mut ch,
            1 as i32 as u64,
        ) == -(1 as i32)
        {
            break;
        }
        i += 1
    }
    wakeup(&mut (*pi).nwrite as *mut u32 as *mut libc::c_void);
    release(&mut (*pi).lock);
    i
}
