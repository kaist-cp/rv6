//! Console input and output, to the uart. Reads are line at a time.
//!
//! Implements special input characters:
//! * newline -- end of line
//! * control-h -- backspace
//! * control-u -- kill line
//! * control-d -- end of file
//! * control-p -- print process list

use core::{fmt, pin::Pin};

use crate::{
    addr::UVAddr,
    arch::interface::{Arch, UartManager, UartManagerConst},
    arch::TargetArch,
    hal::hal,
    kernel::{Kernel, KernelRef},
    lock::{SleepableLock, SleepableLockGuard, SpinLock, SpinLockGuard},
    proc::KernelCtx,
    util::spin_loop,
};

type Uart = <TargetArch as Arch>::Uart;

/// Size of console input buffer.
const INPUT_BUF: usize = 128;
/// Size of console output buffer.
const OUTPUT_BUF: usize = 32;

struct OutputBuffer {
    buf: [u8; OUTPUT_BUF],
    /// Read index.
    w: usize,
    /// Write index.
    r: usize,
}

impl OutputBuffer {
    pub const fn new() -> Self {
        Self {
            buf: [0; OUTPUT_BUF],
            w: 0,
            r: 0,
        }
    }
}

struct InputBuffer {
    buf: [u8; INPUT_BUF],
    /// Read index.
    r: usize,
    /// Write index.
    w: usize,
    /// Edit index.
    e: usize,
}

impl InputBuffer {
    pub const fn new() -> Self {
        Self {
            buf: [0; INPUT_BUF],
            w: 0,
            r: 0,
            e: 0,
        }
    }
}

pub struct Console {
    uart: Uart,
    input_buffer: SleepableLock<InputBuffer>,
    output_buffer: SleepableLock<OutputBuffer>,
}

impl Console {
    /// # Safety
    ///
    /// uart..(uart + 5) are owned addresses.
    pub const unsafe fn new(uart: usize) -> Self {
        Self {
            uart: unsafe { Uart::new(uart) },
            input_buffer: SleepableLock::new("console_input", InputBuffer::new()),
            output_buffer: SleepableLock::new("console_output", OutputBuffer::new()),
        }
    }

    pub fn init(&self) {
        self.uart.init();
    }

    /// Doesn't use interrupts, for use by kernel println() and to echo characters.
    /// It spins waiting for the uart's output register to be empty.
    fn putc_spin<A: Arch>(&self, c: u8, kernel: Pin<&Kernel<A>>) {
        let intr = hal().cpus().push_off();
        if kernel.is_panicked() {
            spin_loop();
        }

        // Wait for Transmit Holding Empty to be set in LSR.
        while self.uart.is_full() {}

        self.uart.putc(c);

        unsafe { hal().cpus().pop_off(intr) };
    }

    fn put_backspace_spin(&self, kernel: Pin<&Kernel<TargetArch>>) {
        // Overwrite with a space.
        self.putc_spin(8, kernel);
        self.putc_spin(b' ', kernel);
        self.putc_spin(8, kernel);
    }

    /// Add a character to the output buffer and tell the UART to start sending if it isn't
    /// already. Blocks if the output buffer is full. Since it may block, it can't be called
    /// from interrupts; it's only suitable for use by write().
    fn putc_sleep(&self, c: u8, ctx: &KernelCtx<'_, '_>) {
        if ctx.kernel().as_ref().is_panicked() {
            spin_loop();
        }

        let mut guard = self.output_buffer.lock();

        while guard.w == guard.r.wrapping_add(OUTPUT_BUF) {
            // Buffer is full.
            // Wait for flush_output_buffer() to open up space in the buffer.
            guard.sleep(ctx);
        }

        let ind = guard.w % OUTPUT_BUF;
        guard.buf[ind] = c;
        guard.w += 1;
        self.flush_output_buffer(guard, ctx.kernel());
    }

    /// If the UART is idle, and a character is waiting in the transmit buffer, send it.
    /// Called from both the top- and bottom-half.
    fn flush_output_buffer(
        &self,
        mut guard: SleepableLockGuard<'_, OutputBuffer>,
        kernel: KernelRef<'_, '_>,
    ) {
        loop {
            if guard.w == guard.r {
                // Transmit buffer is empty.
                return;
            }

            if self.uart.is_full() {
                // The UART transmit holding register is full, so we cannot give it another byte.
                // It will interrupt when it's ready for a new byte.
                return;
            }

            let c = guard.buf[guard.r % OUTPUT_BUF];
            guard.r += 1;

            // Maybe uart.putc() is waiting for space in the buffer.
            guard.wakeup(kernel);

            self.uart.putc(c);
        }
    }

    fn write(&self, src: UVAddr, n: i32, ctx: &mut KernelCtx<'_, '_>) -> i32 {
        for i in 0..n {
            let mut c = [0u8];
            if unsafe {
                ctx.proc()
                    .lock()
                    .deref_mut_info()
                    .memory
                    .assume_init_mut()
                    .copy_in_bytes(&mut c, src + i as usize)
                    .is_err()
            } {
                return i;
            }
            self.putc_sleep(c[0], ctx);
        }
        n
    }

    fn read(&self, mut dst: UVAddr, mut n: i32, ctx: &mut KernelCtx<'_, '_>) -> i32 {
        let mut guard = self.input_buffer.lock();
        let target = n;
        while n > 0 {
            // Wait until interrupt handler has put some
            // input into CONS.buffer.
            while guard.r == guard.w {
                if ctx.proc().killed() {
                    return -1;
                }
                guard.sleep(ctx);
            }
            let cin = guard.buf[guard.r % INPUT_BUF] as i32;
            guard.r = guard.r.wrapping_add(1);

            // end-of-file
            if cin == ctrl('D') {
                if n < target {
                    // Save ^D for next time, to make sure
                    // caller gets a 0-byte result.
                    guard.r = guard.r.wrapping_sub(1)
                }
                break;
            } else {
                // Copy the input byte to the user-space buffer.
                let cbuf = [cin as u8];
                if unsafe {
                    ctx.proc()
                        .lock()
                        .deref_mut_info()
                        .memory
                        .assume_init_mut()
                        .copy_out_bytes(dst, &cbuf)
                        .is_err()
                } {
                    break;
                }
                dst = dst + 1;
                n -= 1;
                if cin == '\n' as i32 {
                    // A whole line has arrived, return to
                    // the user-level read().
                    break;
                }
            }
        }
        target - n
    }

    /// Handle a uart interrupt, raised because input has arrived, or the uart is ready for more
    /// output, or both. Called from trap.c. Do erase/kill processing, append to the input buffer,
    /// and wake up read() if a whole line has arrived.
    ///
    /// # Note
    ///
    /// When `self.uart.getc()` is `Ok(ctrl('P'))`, this method is unsafe.
    pub unsafe fn intr(&self, kernel: KernelRef<'_, '_>) {
        // Read and process incoming characters.
        while let Ok(c) = self.uart.getc() {
            let mut guard = self.input_buffer.lock();
            match c {
                // Print process list.
                m if m == ctrl('P') => {
                    unsafe { kernel.dump() };
                }

                // Kill line.
                m if m == ctrl('U') => {
                    while guard.e != guard.w
                        && guard.buf[guard.e.wrapping_sub(1) % INPUT_BUF] != b'\n'
                    {
                        guard.e = guard.e.wrapping_sub(1);
                        self.put_backspace_spin(kernel.as_ref());
                    }
                }

                // Backspace
                m if m == ctrl('H') | '\x7f' as i32 => {
                    if guard.e != guard.w {
                        guard.e = guard.e.wrapping_sub(1);
                        self.put_backspace_spin(kernel.as_ref());
                    }
                }

                _ => {
                    if c != 0 && guard.e.wrapping_sub(guard.r) < INPUT_BUF {
                        let c = if c == '\r' as i32 { '\n' as i32 } else { c };

                        // Echo back to the user.
                        self.putc_spin(c as u8, kernel.as_ref());

                        // Store for consumption by read().
                        let ind = guard.e % INPUT_BUF;
                        guard.buf[ind] = c as u8;
                        guard.e = guard.e.wrapping_add(1);
                        if c == '\n' as i32
                            || c == ctrl('D')
                            || guard.e == guard.r.wrapping_add(INPUT_BUF)
                        {
                            // Wake up read() if a whole line (or end-of-file) has arrived.
                            guard.w = guard.e;
                            guard.wakeup(kernel);
                        }
                    }
                }
            }
        }

        // Write buffered characters.
        self.flush_output_buffer(self.output_buffer.lock(), kernel);
    }
}

pub struct Printer(SpinLock<()>);

pub struct PrinterGuard<'a, A: Arch> {
    kernel: Pin<&'a Kernel<A>>,
    _guard: Option<SpinLockGuard<'a, ()>>,
}

impl Printer {
    pub const fn new() -> Self {
        Self(SpinLock::new("Printer", ()))
    }

    pub fn lock<'a, A: Arch>(&'a self, kernel: Pin<&'a Kernel<A>>) -> PrinterGuard<'a, A> {
        PrinterGuard {
            kernel,
            _guard: Some(self.0.lock()),
        }
    }

    pub fn without_lock<'a, A: Arch>(&'a self, kernel: Pin<&'a Kernel<A>>) -> PrinterGuard<'a, A> {
        PrinterGuard {
            kernel,
            _guard: None,
        }
    }
}

impl<A: Arch> fmt::Write for PrinterGuard<'_, A> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            hal().console().putc_spin(c, self.kernel);
        }
        Ok(())
    }
}

/// Control-x
const fn ctrl(x: char) -> i32 {
    x as i32 - '@' as i32
}

/// User write()s to the console go here.
pub fn console_write(src: UVAddr, n: i32, ctx: &mut KernelCtx<'_, '_>) -> i32 {
    hal().console().write(src, n, ctx)
}

/// User read()s from the console go here.
/// Copy (up to) a whole input line to dst.
/// User_dist indicates whether dst is a user or kernel address.
pub fn console_read(dst: UVAddr, n: i32, ctx: &mut KernelCtx<'_, '_>) -> i32 {
    hal().console().read(dst, n, ctx)
}
