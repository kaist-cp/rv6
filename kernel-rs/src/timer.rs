use crate::{kernel::KernelRef, proc::KernelCtx};

// pub enum TimingOptions {
//     Tick(u32),
//     Time(Duration),
// }

pub trait TimeManager {
    fn init();

    /// The uptime since power-on of the device.
    ///
    /// This includes time consumed by firmware and bootloaders.
    /// time scale can be different depending on architecture
    fn uptime(kernel: KernelRef<'_, '_>) -> Result<usize, ()>;

    /// Spin for a given duration.
    fn spin_for(kernel: &KernelCtx<'_, '_>, duration: usize) -> Result<(), ()>;

    fn uptime_as_micro() -> Result<usize, ()>;
}
