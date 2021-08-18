use cortex_a::{asm::barrier, registers::*};
use tock_registers::interfaces::{Readable, Writeable};

use crate::{kernel::KernelRef, proc::KernelCtx, timer::TimeManager};

const US_PER_S: u64 = 1_000_000;

const TIMER_TICK_MS: u64 = 100;

pub struct Timer;

impl TimeManager for Timer {
    fn init() {
        Self::set_next_timer();
    }

    /// The uptime since power-on of the device (in number of ticks).
    ///
    /// This includes time consumed by firmware and bootloaders.
    fn uptime<'id, 's>(kernel: KernelRef<'id, 's>) -> Result<usize, ()> {
        Ok(*kernel.ticks().lock() as usize)
    }

    fn spin_for<'id, 's>(kernel_ctx: &KernelCtx<'id, 's>, duration: usize) -> Result<(), ()> {
        let mut ticks = kernel_ctx.kernel().ticks().lock();
        let ticks0 = *ticks;
        while ticks.wrapping_sub(ticks0) < duration as u32 {
            if kernel_ctx.proc().killed() {
                return Err(());
            }
            ticks.sleep(kernel_ctx);
        }
        Ok(())
    }

    fn uptime_as_micro() -> Result<usize, ()> {
        Ok((Timer::read_cntpct() * US_PER_S / Timer::read_freq()) as usize)
    }
}

impl Timer {
    pub fn read_cntpct() -> u64 {
        // Prevent that the counter is read ahead of time due to out-of-order execution.
        unsafe { barrier::isb(barrier::SY) };
        CNTPCT_EL0.get()
    }

    pub fn read_freq() -> u64 {
        unsafe { barrier::isb(barrier::SY) };
        CNTFRQ_EL0.get()
    }

    pub fn set_next_timer() {
        unsafe { barrier::isb(barrier::SY) };
        let freq = CNTFRQ_EL0.get();
        let count = TIMER_TICK_MS * freq / 1000;

        unsafe { barrier::isb(barrier::SY) };
        CNTV_TVAL_EL0.set(count);
        CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE.val(1) + CNTV_CTL_EL0::IMASK.val(0));
        unsafe { barrier::isb(barrier::SY) };
    }

    pub fn udelay(us: u32) {
        let mut current = Self::read_cntpct();
        let condition = current + Self::read_freq() * us as u64 / 1000000;

        while condition > current {
            current = Self::read_cntpct();
        }
    }
}
