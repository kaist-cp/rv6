use super::Armv8;
use crate::arch::interface::PowerOff;

impl PowerOff for Armv8 {
    /// Shutdowns this machine, discarding all unsaved data.
    ///
    /// This function uses SiFive Test Finalizer, which provides power management for QEMU virt device.
    fn machine_poweroff(_exitcode: u16) -> ! {
        todo!("Is there any way to replace this in arm?")
    }
}
