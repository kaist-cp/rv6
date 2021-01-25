use crate::{
    file::{FileType, RcFile},
    kernel::kernel,
    page::Page,
    proc::{myproc, WaitChannel},
    riscv::PGSIZE,
    spinlock::Spinlock,
    vm::UVAddr,
};
use core::{mem, ptr, pin::Pin};

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

#[pin_project]
pub struct Pipe {
    inner: Spinlock<PipeInner>,

    /// WaitChannel for saying there are unread bytes in Pipe.data.
    #[pin]
    read_waitchannel: WaitChannel,

    /// WaitChannel for saying all bytes in Pipe.data are already read.
    #[pin]
    write_waitchannel: WaitChannel,
}

impl Pipe {
    /// PipeInner::try_read() tries to read as much as possible.
    /// Pipe::read() executes try_read() until all bytes in pipe are read.
    //TODO : `n` should be u32
    pub unsafe fn read(self: Pin<&Self>, addr: UVAddr, n: usize) -> Result<usize, ()> {
        let this = self.project_ref();
        let mut inner = this.inner.lock();
        loop {
            match inner.try_read(addr, n) {
                Ok(r) => {
                    //DOC: piperead-wakeup
                    this.write_waitchannel.wakeup();
                    return Ok(r);
                }
                Err(PipeError::WaitForIO) => {
                    //DOC: piperead-sleep
                    this.read_waitchannel.sleep(&mut inner);
                }
                _ => return Err(()),
            }
        }
    }

    /// PipeInner::try_write() tries to write as much as possible.
    /// Pipe::write() executes try_write() until `n` bytes are written.
    pub unsafe fn write(self: Pin<&Self>, addr: UVAddr, n: usize) -> Result<usize, ()> {
        let mut written = 0;
        let this = self.project_ref();
        let mut inner = this.inner.lock();
        loop {
            match inner.try_write(addr + written, n - written) {
                Ok(r) => {
                    written += r;
                    this.read_waitchannel.wakeup();
                    if written < n {
                        this.write_waitchannel.sleep(&mut inner);
                    } else {
                        return Ok(written);
                    }
                }
                Err(PipeError::InvalidCopyin(i)) => {
                    this.read_waitchannel.wakeup();
                    return Ok(written + i);
                }
                _ => return Err(()),
            }
        }
    }

    pub fn close(self: Pin<&Self>, writable: bool) {
        let this = self.project_ref();
        let mut inner = this.inner.lock();

        if writable {
            inner.writeopen = false;
            this.read_waitchannel.wakeup();
        } else {
            inner.readopen = false;
            this.write_waitchannel.wakeup();
        }
    }

    fn closed(self: Pin<&Self>) -> bool {
        let this = self.project_ref();
        let inner = this.inner.lock();

        // Return whether pipe would be freed or not
        !inner.readopen && !inner.writeopen
    }
}

pub struct AllocatedPipe {
    pin: Pin<&'static Pipe>,
}

impl AllocatedPipe {
    pub fn alloc() -> Result<(RcFile<'static>, RcFile<'static>), ()> {
        let page = kernel().alloc().ok_or(())?;
        let ptr = page.into_usize() as *mut Pipe;

        // `Pipe` must be aligned with `Page`.
        const_assert!(mem::size_of::<Pipe>() <= PGSIZE);

        //TODO(rv6): Since Pipe is a huge struct, need to check whether stack is used to fill `*ptr`
        // It is safe because unique access to page is guaranteed since page is just allocated,
        // and the pipe size and alignment are compatible with the page.
        unsafe {
            ptr::write(
                ptr,
                Pipe {
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
                },
            );
        };
        let pin = unsafe {
            // Safe because after this `alloc()` function ends, we can access the `Pipe`
            // only through `AllocatedPipe::pin`, which is `Pin<&Pipe>` type.
            // Hence, the `Pipe` cannot be moved.
            Pin::new_unchecked(&*ptr)
        };
        let f0 = kernel()
            .ftable
            .alloc_file(FileType::Pipe { pipe: Self { pin } }, true, false)
            // It is safe because ptr is an address of page, which obtained by alloc()
            .map_err(|_| kernel().free(unsafe { Page::from_usize(ptr as _) }))?;
        let f1 = kernel()
            .ftable
            .alloc_file(FileType::Pipe { pipe: Self { pin } }, false, true)
            // It is safe because ptr is an address of page, which obtained by alloc()
            .map_err(|_| kernel().free(unsafe { Page::from_usize(ptr as _) }))?;

        Ok((f0, f1))
    }

    pub fn inner(&self) -> Pin<&Pipe> {
        self.pin
    }
}

impl Drop for AllocatedPipe {
    fn drop(&mut self) {
        if self.pin.closed() {
            unsafe {
                // Safe since we won't access the Pipe afterwards after closing.
                let ptr = Pin::into_inner_unchecked(self.pin) as *const Pipe;

                // Safe since this is a page that was allocated through `kernel().alloc()`.
                kernel().free(Page::from_usize(ptr as _));
            }
        }
    }
}

pub enum PipeError {
    WaitForIO,
    InvalidStatus,
    InvalidCopyin(usize),
}

impl PipeInner {
    unsafe fn try_write(&mut self, addr: UVAddr, n: usize) -> Result<usize, PipeError> {
        let mut ch = [0 as u8];
        let proc = myproc();
        if !self.readopen || (*proc).killed() {
            return Err(PipeError::InvalidStatus);
        }
        let data = &mut *(*proc).data.get();
        for i in 0..n {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                return Ok(i);
            }
            if data.pagetable.copy_in(&mut ch, addr + i).is_err() {
                return Err(PipeError::InvalidCopyin(i));
            }
            self.data[self.nwrite as usize % PIPESIZE] = ch[0];
            self.nwrite = self.nwrite.wrapping_add(1);
        }
        Ok(n)
    }

    unsafe fn try_read(&mut self, addr: UVAddr, n: usize) -> Result<usize, PipeError> {
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
            if data.pagetable.copy_out(addr + i, &ch).is_err() {
                return Ok(i);
            }
        }
        Ok(n)
    }
}
