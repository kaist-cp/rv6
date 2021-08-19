use crate::timer::TimeManager;

pub struct Timer;

impl TimeManager for Timer {
    fn init() {
        // nothing to do
    }

    /// The uptime since power-on of the device, in microseconds.
    /// This function is only supporeted on ARM now.
    fn uptime_as_micro() -> Result<usize, ()> {
        todo!()
    }
}
