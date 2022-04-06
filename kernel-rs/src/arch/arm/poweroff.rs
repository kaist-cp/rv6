use super::Armv8;
use crate::arch::asm::{smc_call, SmcFunctions};
use crate::arch::interface::PowerOff;

impl PowerOff for Armv8 {
    /// Shutdowns this machine, discarding all unsaved data.
    ///
    /// This function uses SiFive Test Finalizer, which provides power management for QEMU virt device.
    fn machine_poweroff(_exitcode: u16) -> ! {
        // SAFETY: Valid smc call to turn off the system.
        let _ = unsafe { smc_call(SmcFunctions::SystemOff as u64, 0, 0, 0) };

        unreachable!("Failed to power off machine");
    }
}
