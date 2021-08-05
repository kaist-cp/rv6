use crate::{kernel::KernelRef, proc::KernelCtx, timer::TimeManager};

pub struct Timer;

impl TimeManager for Timer {
    fn init() {
        // nothing to do
    }

    /// The uptime since power-on of the device.
    ///
    /// This includes time consumed by firmware and bootloaders.
    /// Time scale is nanosecond
    fn uptime<'id, 's>(kernel: KernelRef<'id, 's>) -> Result<usize, ()> {
        Ok(*kernel.ticks().lock() as usize)
    }

    /// Spin for a given duration.
    /// Time scale is nanosecond
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
}
