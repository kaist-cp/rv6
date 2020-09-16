use crate::libc;
use crate::{
    file::{FileType, RcFile},
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
    pub unsafe fn read(&self, addr: usize, n: i32) -> Result<usize, ()> {
        let mut inner = self.inner.lock();
        loop {
            match inner.try_read(addr, n) {
                Ok(r) => {
                    //DOC: piperead-wakeup
                    self.write_waitchannel.wakeup();
                    return Ok(r);
                }
                Err(PipeError::WaitForIO) => {
                    //DOC: piperead-sleep
                    self.read_waitchannel.sleep(inner.raw() as _);
                }
                _ => return Err(()),
            }
        }
    }

    /// PipeInner::try_write() tries to write as much as possible.
    /// Pipe::write() executes try_write() until `n` bytes are written.
    pub unsafe fn write(&self, addr: usize, n: i32) -> Result<usize, ()> {
        let mut written: i32 = 0;
        let mut inner = self.inner.lock();
        loop {
            match inner.try_write(addr + written as usize, n - written) {
                Ok(r) => {
                    written += r as i32;
                    self.read_waitchannel.wakeup();
                    if written < n {
                        self.write_waitchannel.sleep(inner.raw() as _);
                    } else {
                        return Ok(written as usize);
                    }
                }
                _ => return Err(()),
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

    pub unsafe fn alloc() -> Result<(RcFile, RcFile), ()> {
        let mut f0 = RcFile::alloc().ok_or(())?;
        let mut f1 = RcFile::alloc().ok_or(())?;

        let ptr = kalloc() as *mut Pipe;
        if ptr.is_null() {
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

        // FIXME: You cannot get the mutable reference of Rc.
        (*f0).typ = FileType::Pipe { pipe: Self { ptr } };
        (*f0).set_readable(true);
        (*f0).set_writable(false);
        (*f1).typ = FileType::Pipe { pipe: Self { ptr } };
        (*f1).set_readable(false);
        (*f1).set_writable(true);

        Ok((f0, f1))
    }

    // TODO: use `Drop` instead of `close`
    pub unsafe fn close(&mut self, writable: bool) {
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
    unsafe fn try_write(&mut self, addr: usize, n: i32) -> Result<usize, ()> {
        let mut ch: u8 = 0;
        let proc = myproc();
        for i in 0..n as usize {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if !self.readopen || (*proc).killed {
                    return Err(());
                }
                return Ok(i);
            }
            if copyin((*proc).pagetable, &mut ch, addr.wrapping_add(i), 1usize) == -1 {
                break;
            }
            self.data[self.nwrite as usize % PIPESIZE] = ch;
            self.nwrite = self.nwrite.wrapping_add(1);
        }
        Ok(n as usize)
    }

    unsafe fn try_read(&mut self, addr: usize, n: i32) -> Result<usize, PipeError> {
        let proc = myproc();

        //DOC: pipe-empty
        if self.nread == self.nwrite && self.writeopen {
            if (*proc).killed {
                return Err(PipeError::InvalidStatus);
            }
            return Err(PipeError::WaitForIO);
        }

        //DOC: piperead-copy
        for i in 0..n as usize {
            if self.nread == self.nwrite {
                return Ok(i);
            }
            let mut ch = self.data[self.nread as usize % PIPESIZE];
            self.nread = self.nread.wrapping_add(1);
            if copyout((*proc).pagetable, addr.wrapping_add(i), &mut ch, 1usize) == -1 {
                return Ok(i);
            }
        }
        Ok(n as usize)
    }
}
