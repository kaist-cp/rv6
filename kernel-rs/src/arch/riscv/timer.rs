use crate::{kernel::KernelRef, proc::KernelCtx, timer::TimeManager};

pub struct Timer;

impl TimeManager for Timer {
    fn init() {
        // nothing to do
    }

    /// The uptime since power-on of the device.
    ///
    /// This includes time consumed by firmware and bootloaders.
    fn uptime(kernel: KernelRef<'_, '_>) -> Result<usize, ()> {
        Ok(*kernel.ticks().lock() as usize)
    }

    /// Spin for a given duration.
    fn spin_for(kernel_ctx: &KernelCtx<'_, '_>, duration: usize) -> Result<(), ()> {
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

    /// The uptime since power-on of the device, in microseconds.
    /// This function is only supporeted on ARM now.
    fn uptime_as_micro() -> Result<usize, ()> {
        todo!()
    }
}
