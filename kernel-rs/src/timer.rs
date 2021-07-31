use crate::{kernel::KernelRef, proc::KernelCtx};

// pub enum TimingOptions {
//     Tick(u32),
//     Time(Duration),
// }

pub trait TimeManager {
    /// The uptime since power-on of the device.
    ///
    /// This includes time consumed by firmware and bootloaders.
    /// time scale can be different depending on architecture
    fn uptime<'id, 's>(kernel: KernelRef<'id, 's>) -> Result<usize, ()>;

    /// Spin for a given duration.
    fn spin_for<'id, 's>(kernel: &KernelCtx<'id, 's>, duration: usize) -> Result<(), ()>;
}
