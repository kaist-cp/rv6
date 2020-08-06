use crate::libc;
use crate::{
    file::{filealloc, File},
    kalloc::{kalloc, kfree},
    proc::{myproc, proc_0, sleep, wakeup},
    spinlock::Spinlock,
    vm::{copyin, copyout},
};
use core::ptr;

pub const PIPESIZE: i32 = 512;

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

impl Pipe {
    pub unsafe fn close(&mut self, mut writable: i32) {
        (*self).lock.acquire();
        if writable != 0 {
            (*self).writeopen = 0 as i32;
            wakeup(&mut (*self).nread as *mut u32 as *mut libc::c_void);
        } else {
            (*self).readopen = 0 as i32;
            wakeup(&mut (*self).nwrite as *mut u32 as *mut libc::c_void);
        }
        if (*self).readopen == 0 as i32 && (*self).writeopen == 0 as i32 {
            (*self).lock.release();
            kfree(self as *mut Pipe as *mut libc::c_char as *mut libc::c_void);
        } else {
            (*self).lock.release();
        };
    }
    pub unsafe fn write(&mut self, mut addr: u64, mut n: i32) -> i32 {
        let mut i: i32 = 0;
        let mut ch: libc::c_char = 0;
        let mut pr: *mut proc_0 = myproc();
        (*self).lock.acquire();
        while i < n {
            while (*self).nwrite == (*self).nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if (*self).readopen == 0 as i32 || (*myproc()).killed != 0 {
                    (*self).lock.release();
                    return -(1 as i32);
                }
                wakeup(&mut (*self).nread as *mut u32 as *mut libc::c_void);
                sleep(
                    &mut (*self).nwrite as *mut u32 as *mut libc::c_void,
                    &mut (*self).lock,
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
            let fresh0 = (*self).nwrite;
            (*self).nwrite = (*self).nwrite.wrapping_add(1);
            (*self).data[fresh0.wrapping_rem(PIPESIZE as u32) as usize] = ch;
            i += 1
        }
        wakeup(&mut (*self).nread as *mut u32 as *mut libc::c_void);
        (*self).lock.release();
        n
    }
    pub unsafe fn read(&mut self, mut addr: u64, mut n: i32) -> i32 {
        let mut i: i32 = 0;
        let mut pr: *mut proc_0 = myproc();
        let mut ch: libc::c_char = 0;

        (*self).lock.acquire();

        //DOC: pipe-empty
        while (*self).nread == (*self).nwrite && (*self).writeopen != 0 {
            if (*myproc()).killed != 0 {
                (*self).lock.release();
                return -(1 as i32);
            }

            //DOC: piperead-sleep
            sleep(
                &mut (*self).nread as *mut u32 as *mut libc::c_void,
                &mut (*self).lock,
            );
        }

        //DOC: piperead-copy
        while i < n {
            if (*self).nread == (*self).nwrite {
                break;
            }
            let fresh1 = (*self).nread;
            (*self).nread = (*self).nread.wrapping_add(1);
            ch = (*self).data[fresh1.wrapping_rem(PIPESIZE as u32) as usize];
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

        //DOC: piperead-wakeup
        wakeup(&mut (*self).nwrite as *mut u32 as *mut libc::c_void);
        (*self).lock.release();
        i
    }
}

pub unsafe fn pipealloc(mut f0: *mut *mut File, mut f1: *mut *mut File) -> i32 {
    let mut pi: *mut Pipe = ptr::null_mut();
    pi = ptr::null_mut();
    *f1 = ptr::null_mut();
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
            (*pi)
                .lock
                .initlock(b"pipe\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
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
        (*(*f0)).close();
    }
    if !(*f1).is_null() {
        (*(*f1)).close();
    }
    -(1 as i32)
}
