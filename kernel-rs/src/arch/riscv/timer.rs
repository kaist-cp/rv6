use super::RiscV;
use crate::arch::interface::TimeManager;

impl TimeManager for RiscV {
    fn timer_init() {
        // nothing to do
    }

    /// The uptime since power-on of the device, in microseconds.
    /// This function is only supporeted on ARM now.
    fn uptime_as_micro() -> Result<usize, ()> {
        todo!()
    }
}
