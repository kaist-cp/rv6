use crate::{
    file::{FileType, RcFile},
    kernel::kernel,
    proc::{myproc, WaitChannel},
    spinlock::Spinlock,
    vm::{UVAddr, VAddr},
};
use core::ops::Deref;

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
    pub unsafe fn read(&self, addr: usize, n: usize) -> Result<usize, ()> {
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
                    self.read_waitchannel.sleep(&mut inner);
                }
                _ => return Err(()),
            }
        }
    }

    /// PipeInner::try_write() tries to write as much as possible.
    /// Pipe::write() executes try_write() until `n` bytes are written.
    pub unsafe fn write(&self, addr: usize, n: usize) -> Result<usize, ()> {
        let mut written = 0;
        let mut inner = self.inner.lock();
        loop {
            match inner.try_write(addr + written, n - written) {
                Ok(r) => {
                    written += r;
                    self.read_waitchannel.wakeup();
                    if written < n {
                        self.write_waitchannel.sleep(&mut inner);
                    } else {
                        return Ok(written);
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
    pub unsafe fn alloc() -> Result<(RcFile, RcFile), ()> {
        let ptr = kernel().alloc() as *mut Pipe;
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
        let f0 = RcFile::alloc(FileType::Pipe { pipe: Self { ptr } }, true, false)
            .ok_or_else(|| kernel().free(ptr as _))?;
        let f1 = RcFile::alloc(FileType::Pipe { pipe: Self { ptr } }, false, true)
            .ok_or_else(|| kernel().free(ptr as _))?;

        Ok((f0, f1))
    }

    // TODO: use `Drop` instead of `close`
    // TODO: use `self` instead of `&mut self`
    // `&mut self` is used because `Drop` of `File` uses AllocatedPipe inside File.
    // https://github.com/kaist-cp/rv6/pull/211#discussion_r491671723
    pub unsafe fn close(&mut self, writable: bool) {
        if (*self.ptr).close(writable) {
            kernel().free(self.ptr as *mut Pipe as _);
        }
    }
}

pub enum PipeError {
    WaitForIO,
    InvalidStatus,
}

impl PipeInner {
    unsafe fn try_write(&mut self, addr: usize, n: usize) -> Result<usize, ()> {
        let mut ch = [0 as u8];
        let proc = myproc();
        let data = &mut *(*proc).data.get();

        for i in 0..n {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                if !self.readopen || (*proc).killed() {
                    return Err(());
                }
                return Ok(i);
            }
            if data
                .pagetable
                .assume_init_mut()
                .copyin(&mut ch, UVAddr::new(addr.wrapping_add(i)))
                .is_err()
            {
                break;
            }
            self.data[self.nwrite as usize % PIPESIZE] = ch[0];
            self.nwrite = self.nwrite.wrapping_add(1);
        }
        Ok(n)
    }

    unsafe fn try_read(&mut self, addr: usize, n: usize) -> Result<usize, PipeError> {
        let proc = myproc();
        let data = &mut *(*proc).data.get();

        //DOC: pipe-empty
        if self.nread == self.nwrite && self.writeopen {
            if (*proc).killed() {
                return Err(PipeError::InvalidStatus);
            }
            return Err(PipeError::WaitForIO);
        }

        //DOC: piperead-copy
        for i in 0..n {
            if self.nread == self.nwrite {
                return Ok(i);
            }
            let ch = [self.data[self.nread as usize % PIPESIZE]];
            self.nread = self.nread.wrapping_add(1);
            if data
                .pagetable
                .assume_init_mut()
                .copyout(UVAddr::new(addr.wrapping_add(i)), &ch)
                .is_err()
            {
                return Ok(i);
            }
        }
        Ok(n)
    }
}
