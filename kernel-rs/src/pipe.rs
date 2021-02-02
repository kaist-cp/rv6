use crate::{
    file::{FileType, RcFile},
    kernel::kernel,
    page::Page,
    proc::{Proc, WaitChannel},
    riscv::PGSIZE,
    spinlock::Spinlock,
    vm::UVAddr,
};
use core::{mem, ops::Deref, ptr::NonNull};
use static_assertions::const_assert;

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
    /// Tries to read up to `n` bytes using `Pipe::try_read()`.
    /// If successfully read i > 0 bytes, wakeups the `write_waitchannel` and returns `Ok(i: usize)`.
    /// If the pipe was empty, sleeps at `read_waitchannel` and tries again after wakeup.
    /// If an error happened, returns `Err(())`.
    pub fn read(&self, addr: UVAddr, n: usize, proc: &mut Proc) -> Result<usize, ()> {
        let mut inner = self.inner.lock();
        loop {
            match inner.try_read(addr, n, proc) {
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

    /// Tries to write up to `n` bytes by repeatedly calling `Pipe::try_write()`.
    /// Wakeups `read_waitchannel` for every successful `Pipe::try_write()`.
    /// After successfully writing i >= 0 bytes, returns `Ok(i)`.
    /// Note that we may have i < `n` if an copy-in error happened.
    /// If the pipe was full, sleeps at `write_waitchannel` and tries again after wakeup.
    /// If an error happened, returns `Err(())`.
    pub fn write(&self, addr: UVAddr, n: usize, proc: &mut Proc) -> Result<usize, ()> {
        let mut written = 0;
        let mut inner = self.inner.lock();
        loop {
            match inner.try_write(addr + written, n - written, proc) {
                Ok(r) => {
                    written += r;
                    self.read_waitchannel.wakeup();
                    if written < n {
                        self.write_waitchannel.sleep(&mut inner);
                    } else {
                        return Ok(written);
                    }
                }
                Err(PipeError::InvalidCopyin(i)) => {
                    self.read_waitchannel.wakeup();
                    return Ok(written + i);
                }
                _ => return Err(()),
            }
        }
    }

    fn close(&self, writable: bool) -> bool {
        let mut inner = self.inner.lock();

        if writable {
            inner.writeopen = false;
            self.read_waitchannel.wakeup();
        } else {
            inner.readopen = false;
            self.write_waitchannel.wakeup();
        }

        // Return whether pipe should be freed or not.
        !inner.readopen && !inner.writeopen
    }
}

/// # Safety
///
/// `ptr` always refers to a `Pipe`.
/// Also, for a single `Pipe`, we have a single read-only `AllocatedPipe` and a single write-only `AllocatedPipe`.
/// The `PipeInner`'s readopen/writeopen field denotes whether the read-only/write-only `AllocatedPipe` is still open,
/// and hence, we can safely free the `Pipe` only after both the readopen/writeopen field is false, since this means
/// all `AllocatedPipe`s were closed.
pub struct AllocatedPipe {
    ptr: NonNull<Pipe>,
}

// `AllocatedPipe` is `Send` because we access `PipeInner` only after acquring a lock
// and because `AllocatedPipe` does not point to thread-local data.
unsafe impl Send for AllocatedPipe {}

impl Deref for AllocatedPipe {
    type Target = Pipe;
    fn deref(&self) -> &Self::Target {
        // Safe since `ptr` always refers to a `Pipe`.
        unsafe { self.ptr.as_ref() }
    }
}

impl AllocatedPipe {
    pub fn alloc() -> Result<(RcFile<'static>, RcFile<'static>), ()> {
        let page = kernel().alloc().ok_or(())?;
        let mut ptr = unsafe {
            // Safe since by the invariant of `Page`, `page` is always non-null.
            NonNull::new_unchecked(page.into_usize() as *mut Pipe)
        };

        // `Pipe` must be aligned with `Page`.
        const_assert!(mem::size_of::<Pipe>() <= PGSIZE);

        //TODO(https://github.com/kaist-cp/rv6/issues/367): Since Pipe is a huge struct, need to check whether stack is used to fill `*ptr`.
        unsafe {
            // Safe since `ptr` holds a valid, unique page allocated from `kernel().alloc()`,
            // and the pipe size and alignment are compatible with the page.
            *ptr.as_mut() = Pipe {
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
        }
        let f0 = kernel()
            .ftable
            .alloc_file(FileType::Pipe { pipe: Self { ptr } }, true, false)
            // Safe since ptr is an address of a page obtained by alloc().
            .map_err(|_| kernel().free(unsafe { Page::from_usize(ptr.as_ptr() as _) }))?;
        let f1 = kernel()
            .ftable
            .alloc_file(FileType::Pipe { pipe: Self { ptr } }, false, true)
            // Safe since ptr is an address of a page obtained by alloc().
            .map_err(|_| kernel().free(unsafe { Page::from_usize(ptr.as_ptr() as _) }))?;

        Ok((f0, f1))
    }

    pub fn close(self, writable: bool) {
        if self.deref().close(writable) {
            // If `Pipe::close()` returned true, this means all `AllocatedPipe`s were closed.
            // Hence, we can free the `Pipe`.
            // Also, the following is safe since `ptr` holds a `Pipe` stored in a valid page allocated from `kernel().alloc()`.
            kernel().free(unsafe { Page::from_usize(self.ptr.as_ptr() as _) });
        }
    }
}

pub enum PipeError {
    WaitForIO,
    InvalidStatus,
    InvalidCopyin(usize),
}

impl PipeInner {
    /// Tries to write up to `n` bytes.
    /// If the process was killed, returns `Err(InvalidStatus)`.
    /// If an copy-in error happened after successfully writing i >= 0 bytes, returns `Err(InvalidCopyIn(i))`.
    /// Otherwise, returns `Ok(i)` after successfully writing i >= 0 bytes.    
    fn try_write(&mut self, addr: UVAddr, n: usize, proc: &mut Proc) -> Result<usize, PipeError> {
        let mut ch = [0u8];
        if !self.readopen || proc.killed() {
            return Err(PipeError::InvalidStatus);
        }
        for i in 0..n {
            if self.nwrite == self.nread.wrapping_add(PIPESIZE as u32) {
                //DOC: pipewrite-full
                return Ok(i);
            }
            if proc.memory.copy_in(&mut ch, addr + i).is_err() {
                return Err(PipeError::InvalidCopyin(i));
            }
            self.data[self.nwrite as usize % PIPESIZE] = ch[0];
            self.nwrite = self.nwrite.wrapping_add(1);
        }
        Ok(n)
    }

    /// Tries to read up to `n` bytes.
    /// If successful read i > 0 bytes, returns `Ok(i: usize)`.
    /// If the pipe was empty, returns `Err(WaitForIO)`.
    /// If the process was killed, returns `Err(InvalidStatus)`.
    fn try_read(&mut self, addr: UVAddr, n: usize, proc: &mut Proc) -> Result<usize, PipeError> {
        //DOC: pipe-empty
        if self.nread == self.nwrite && self.writeopen {
            if proc.killed() {
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
            if proc.memory.copy_out(addr + i, &ch).is_err() {
                return Ok(i);
            }
        }
        Ok(n)
    }
}
