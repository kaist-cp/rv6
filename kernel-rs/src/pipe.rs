use crate::libc;
use crate::{
    file::{File, Filetype},
    kalloc::{kalloc, kfree},
    proc::{myproc, WaitChannel},
    spinlock::RawSpinlock,
    vm::{copyin, copyout},
};
use core::ptr;

const PIPESIZE: usize = 512;

pub struct Pipe {
    lock: RawSpinlock,
    data: [u8; PIPESIZE],

    /// Number of bytes read.
    nread: u32,

    /// Number of bytes written.
    nwrite: u32,

    /// Read fd is still open.
    readopen: bool,

    /// Write fd is still open.
    writeopen: bool,

    /// WaitChannel for saying there are unread bytes in Pipe.data.
    read_waitchannel: WaitChannel,

    /// WaitChannel for saying all bytes in Pipe.data are already read.
    write_waitchannel: WaitChannel,
}

impl Pipe {
    pub unsafe fn close(&mut self, writable: bool) {
        self.lock.acquire();
        if writable {
            self.writeopen = false;
            self.read_waitchannel.wakeup();
        } else {
            self.readopen = false;
            self.write_waitchannel.wakeup();
        }
        if !self.readopen && !self.writeopen {
            self.lock.release();
            kfree(self as *mut Pipe as *mut u8 as *mut libc::CVoid);
        } else {
            self.lock.release();
        };
    }
    pub unsafe fn write(&mut self, addr: usize, n: i32) -> i32 {
        let mut i = 0;
        let mut ch: u8 = 0;
        let proc = myproc();
        self.lock.acquire();
        while i < n {
            while self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if !self.readopen || (*myproc()).killed {
                    self.lock.release();
                    return -1;
                }
                self.read_waitchannel.wakeup();
                self.write_waitchannel.sleep(&mut self.lock);
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
            let fresh0 = self.nwrite;
            self.nwrite = self.nwrite.wrapping_add(1);
            self.data[(fresh0 as usize).wrapping_rem(PIPESIZE)] = ch;
            i += 1
        }
        self.read_waitchannel.wakeup();
        self.lock.release();
        n
    }
    pub unsafe fn read(&mut self, addr: usize, n: i32) -> i32 {
        let mut i = 0;
        let proc = myproc();

        self.lock.acquire();

        //DOC: pipe-empty
        while self.nread == self.nwrite && self.writeopen {
            if (*myproc()).killed {
                self.lock.release();
                return -1;
            }

            //DOC: piperead-sleep
            self.read_waitchannel.sleep(&mut self.lock);
        }

        //DOC: piperead-copy
        while i < n {
            if self.nread == self.nwrite {
                break;
            }
            let fresh1 = self.nread;
            self.nread = self.nread.wrapping_add(1);
            let mut ch: u8 = self.data[(fresh1 as usize).wrapping_rem(PIPESIZE)];
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
        self.write_waitchannel.wakeup();
        self.lock.release();
        i
    }
    //TODO : make alloc() return Result<(*mut File, *mut File), ()>
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
                (*pi).readopen = true;
                (*pi).writeopen = true;
                (*pi).nwrite = 0;
                (*pi).nread = 0;
                (*pi).lock.initlock("pipe");
                (*pi).read_waitchannel = WaitChannel::new();
                (*pi).write_waitchannel = WaitChannel::new();
                (**f0).typ = Filetype::PIPE;
                (**f0).readable = true;
                (**f0).writable = false;
                (**f0).pipe = pi;
                (**f1).typ = Filetype::PIPE;
                (**f1).readable = false;
                (**f1).writable = true;
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
