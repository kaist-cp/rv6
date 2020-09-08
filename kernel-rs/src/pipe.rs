use crate::libc;
use crate::{
    file::{File, Filetype},
    kalloc::{kalloc, kfree},
    proc::{myproc, WaitChannel},
    spinlock::Spinlock,
    vm::{copyin, copyout},
};
use core::{ops::Deref, ptr};

const PIPESIZE: usize = 512;

struct PipeInner {
    data: [u8; PIPESIZE],

    /// Number of bytes read.
    nread: u32,

    /// Number of bytes written.
    nwrite: u32,

    /// Read fd is still open.
    readopen: bool,

    /// Write fd is still open.
    writeopen: bool,
}

pub struct Pipe {
    inner: Spinlock<PipeInner>,

    /// WaitChannel for saying there are unread bytes in Pipe.data.
    read_waitchannel: WaitChannel,

    /// WaitChannel for saying all bytes in Pipe.data are already read.
    write_waitchannel: WaitChannel,
}

impl Pipe {
    pub unsafe fn read(&self, addr: usize, n: i32) -> i32 {
        loop {
            let mut inner = self.inner.lock();
            match inner.try_read(addr, n) {
                Ok(r) => {
                    //DOC: piperead-wakeup
                    self.write_waitchannel.wakeup();
                    return r;
                }
                Err(PipeError::WaitForIO) => {
                    //DOC: piperead-sleep
                    self.read_waitchannel.sleep(inner.raw() as _);
                }
                _ => return -1,
            }
        }
    }
    pub unsafe fn write(&self, addr: usize, n: i32) -> i32 {
        loop {
            let mut inner = self.inner.lock();
            match inner.try_write(addr, n) {
                Ok(r) => {
                    self.read_waitchannel.wakeup();
                    return r;
                }
                Err(PipeError::WaitForIO) => {
                    self.read_waitchannel.wakeup();
                    self.write_waitchannel.sleep(inner.raw() as _);
                }
                _ => return -1,
            }
        }
    }

    unsafe fn close(&mut self, writable: bool) -> bool {
        let mut inner = self.inner.lock();

        if writable {
            inner.writeopen = false;
            self.read_waitchannel.wakeup();
        } else {
            inner.readopen = false;
            self.write_waitchannel.wakeup();
        }

        !inner.readopen && !inner.writeopen
    }
}

#[derive(Copy, Clone)]
pub struct AllocatedPipe {
    ptr: *mut Pipe,
}

impl Deref for AllocatedPipe {
    type Target = Pipe;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl AllocatedPipe {
    pub const fn zeroed() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }

    pub unsafe fn alloc() -> Result<(*mut File, *mut File), ()> {
        let f0 = File::alloc();
        if f0.is_null() {
            return Err(());
        }

        let f1 = File::alloc();
        if f1.is_null() {
            (*f0).close();
            return Err(());
        }

        let ptr = kalloc() as *mut Pipe;
        if ptr.is_null() {
            (*f0).close();
            (*f1).close();
            return Err(());
        }

        //TODO: Since Pipe is a huge struct, need to check whether stack is used to fill `*ptr`
        *ptr = Pipe {
            inner: Spinlock::new(
                "pipe",
                PipeInner {
                    data: [0; PIPESIZE],
                    nwrite: 0,
                    nread: 0,
                    readopen: true,
                    writeopen: true,
                },
            ),
            read_waitchannel: WaitChannel::new(),
            write_waitchannel: WaitChannel::new(),
        };

        (*f0).typ = Filetype::PIPE;
        (*f0).readable = true;
        (*f0).writable = false;
        (*f0).pipe = Self { ptr };
        (*f1).typ = Filetype::PIPE;
        (*f1).readable = false;
        (*f1).writable = true;
        (*f1).pipe = Self { ptr };

        Ok((f0, f1))
    }

    pub unsafe fn close(&self, writable: bool) {
        if (*self.ptr).close(writable) {
            kfree(self.ptr as *mut Pipe as *mut u8 as *mut libc::CVoid);
        }
    }
}

pub enum PipeError {
    WaitForIO,
    InvalidStatus,
}

impl PipeInner {
    unsafe fn try_write(&mut self, addr: usize, n: i32) -> Result<i32, PipeError> {
        let mut ch: u8 = 0;
        let proc = myproc();
        for i in 0..n {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if !self.readopen || (*myproc()).killed {
                    return Err(PipeError::InvalidStatus);
                }
                return Err(PipeError::WaitForIO);
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
            self.data[self.nwrite as usize % PIPESIZE] = ch;
            self.nwrite = self.nwrite.wrapping_add(1);
        }
        Ok(n)
    }
    unsafe fn try_read(&mut self, addr: usize, n: i32) -> Result<i32, PipeError> {
        let proc = myproc();

        //DOC: pipe-empty
        if self.nread == self.nwrite && self.writeopen {
            if (*myproc()).killed {
                return Err(PipeError::InvalidStatus);
            }
            return Err(PipeError::WaitForIO);
        }

        //DOC: piperead-copy
        for i in 0..n {
            if self.nread == self.nwrite {
                return Ok(i);
            }
            let mut ch: u8 = self.data[self.nread as usize % PIPESIZE];
            self.nread = self.nread.wrapping_add(1);
            if copyout(
                (*proc).pagetable,
                addr.wrapping_add(i as usize),
                &mut ch,
                1usize,
            ) == -1
            {
                return Ok(i);
            }
        }
        Ok(n)
    }
}
