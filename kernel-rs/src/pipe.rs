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
    /// PipeInner::try_read() tries to read as much as possible.
    /// Pipe::read() executes try_read() until all bytes in pipe are read.
    //TODO : `n` should be u32
    pub unsafe fn read(&self, addr: usize, n: i32) -> i32 {
        loop {
            let mut inner = self.inner.lock();
            match inner.try_read(addr, n) {
                Ok(r) => {
                    if r < 0 {
                        //DOC: piperead-sleep
                        self.read_waitchannel.sleep(inner.raw() as _);
                    } else {
                        //DOC: piperead-wakeup
                        self.write_waitchannel.wakeup();
                        return r;
                    }
                }
                _ => return -1,
            }
        }
    }

    /// PipeInner::try_write() tries to write as much as possible.
    /// Pipe::write() executes try_write() until `n` bytes are written.
    pub unsafe fn write(&self, addr: usize, n: i32) -> i32 {
        let mut written: i32 = 0;
        loop {
            let mut inner = self.inner.lock();
            match inner.try_write(addr + written as usize, n - written) {
                Ok(r) => {
                    written += r;
                    if written < n {
                        self.read_waitchannel.wakeup();
                        self.write_waitchannel.sleep(inner.raw() as _);
                    } else {
                        self.read_waitchannel.wakeup();
                        return written;
                    }
                }
                _ => return -1,
            }
        }
    }

    unsafe fn close(&self, writable: bool) -> bool {
        let mut inner = self.inner.lock();

        if writable {
            inner.writeopen = false;
            self.read_waitchannel.wakeup();
        } else {
            inner.readopen = false;
            self.write_waitchannel.wakeup();
        }

        // Return whether pipe would be freed or not
        !inner.readopen && !inner.writeopen
    }
}

// TODO: Remove Copy and Clone
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

    // TODO: use `Drop` instead of `close`
    pub unsafe fn close(&mut self, writable: bool) {
        if (*self.ptr).close(writable) {
            kfree(self.ptr as *mut Pipe as *mut u8 as *mut libc::CVoid);
        }
    }
}

impl PipeInner {
    unsafe fn try_write(&mut self, addr: usize, n: i32) -> Result<i32, ()> {
        let mut ch: u8 = 0;
        let proc = myproc();
        for i in 0..n {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if !self.readopen || (*myproc()).killed {
                    return Err(());
                }
                return Ok(i);
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
    unsafe fn try_read(&mut self, addr: usize, n: i32) -> Result<i32, ()> {
        let proc = myproc();

        //DOC: pipe-empty
        if self.nread == self.nwrite && self.writeopen {
            if (*myproc()).killed {
                return Err(());
            }
            return Ok(-1);
        }

        //DOC: piperead-copy
        for i in 0..n {
            if self.nread == self.nwrite {
                return Ok(i);
            }
            let mut ch = self.data[self.nread as usize % PIPESIZE];
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
