pub trait TimeManager {
    fn init();
    
    /// The uptime since power-on of the device, in microseconds.
    /// This includes time consumed by firmware and bootloaders.
    fn uptime_as_micro() -> Result<usize, ()>;
}
