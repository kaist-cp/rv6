use crate::libc;
use crate::{
    file::File,
    fs::FD_PIPE,
    kalloc::{kalloc, kfree},
    proc::{myproc, Proc, WaitChannel},
    spinlock::RawSpinlock,
    vm::{copyin, copyout},
};
use core::ptr;

pub const PIPESIZE: usize = 512;

pub struct Pipe {
    pub lock: RawSpinlock,
    pub data: [u8; PIPESIZE],

    /// number of bytes read
    pub nread: u32,

    /// number of bytes written
    pub nwrite: u32,

    /// read fd is still open
    pub readopen: i32,

    /// write fd is still open
    pub writeopen: i32,

    readwaitchan: WaitChannel,
    writewaitchan: WaitChannel,
}

impl Pipe {
    pub unsafe fn close(&mut self, writable: i32) {
        (*self).lock.acquire();
        if writable != 0 {
            (*self).writeopen = 0;
            self.readwaitchan.wakeup();
        } else {
            (*self).readopen = 0;
            self.writewaitchan.wakeup();
        }
        if (*self).readopen == 0 && (*self).writeopen == 0 {
            (*self).lock.release();
            kfree(self as *mut Pipe as *mut u8 as *mut libc::CVoid);
        } else {
            (*self).lock.release();
        };
    }
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> i32 {
        let mut i: i32 = 0;
        let mut ch: u8 = 0;
        let proc: *mut Proc = myproc();
        (*self).lock.acquire();
        while i < n {
            while (*self).nwrite == (*self).nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if (*self).readopen == 0 || (*myproc()).killed != 0 {
                    (*self).lock.release();
                    return -1;
                }
                self.readwaitchan.wakeup();
                self.writewaitchan.sleep(&mut (*self).lock);
            }
            if copyin(
                (*proc).pagetable,
                &mut ch,
                addr.wrapping_add(i as usize),
                1usize,
            ) == -1
            {
                break;
            }
            let fresh0 = (*self).nwrite;
            (*self).nwrite = (*self).nwrite.wrapping_add(1);
            (*self).data[(fresh0 as usize).wrapping_rem(PIPESIZE)] = ch;
            i += 1
        }
        self.readwaitchan.wakeup();
        (*self).lock.release();
        n
    }
    pub unsafe fn read(&mut self, addr: usize, n: i32) -> i32 {
        let mut i: i32 = 0;
        let proc: *mut Proc = myproc();

        (*self).lock.acquire();

        //DOC: pipe-empty
        while (*self).nread == (*self).nwrite && (*self).writeopen != 0 {
            if (*myproc()).killed != 0 {
                (*self).lock.release();
                return -1;
            }

            //DOC: piperead-sleep
            self.readwaitchan.sleep(&mut (*self).lock);
        }

        //DOC: piperead-copy
        while i < n {
            if (*self).nread == (*self).nwrite {
                break;
            }
            let fresh1 = (*self).nread;
            (*self).nread = (*self).nread.wrapping_add(1);
            let mut ch: u8 = (*self).data[(fresh1 as usize).wrapping_rem(PIPESIZE)];
            if copyout(
                (*proc).pagetable,
                addr.wrapping_add(i as usize),
                &mut ch,
                1usize,
            ) == -1
            {
                break;
            }
            i += 1
        }

        //DOC: piperead-wakeup
        self.writewaitchan.wakeup();
        (*self).lock.release();
        i
    }
    pub unsafe fn alloc(mut f0: *mut *mut File, mut f1: *mut *mut File) -> i32 {
        let mut pi: *mut Pipe = ptr::null_mut();
        *f1 = ptr::null_mut();
        *f0 = *f1;
        *f0 = File::alloc();
        if !((*f0).is_null() || {
            *f1 = File::alloc();
            (*f1).is_null()
        }) {
            pi = kalloc() as *mut Pipe;
            if !pi.is_null() {
                (*pi).readopen = 1;
                (*pi).writeopen = 1;
                (*pi).nwrite = 0;
                (*pi).nread = 0;
                (*pi).lock.initlock("pipe");
                (**f0).typ = FD_PIPE;
                (**f0).readable = 1;
                (*pi).readwaitchan = WaitChannel::new();
                (**f0).writable = 0;
                (*pi).writewaitchan = WaitChannel::new();
                (**f0).pipe = pi;
                (**f1).typ = FD_PIPE;
                (**f1).readable = 0;
                (**f1).writable = 1;
                (**f1).pipe = pi;
                return 0;
            }
        }
        if !pi.is_null() {
            kfree(pi as *mut u8 as *mut libc::CVoid);
        }
        if !(*f0).is_null() {
            (*(*f0)).close();
        }
        if !(*f1).is_null() {
            (*(*f1)).close();
        }
        -1
    }
}
