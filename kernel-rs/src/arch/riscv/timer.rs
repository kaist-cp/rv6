use core::arch::asm;

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

    fn r_cycle() -> usize {
        let mut x;
        unsafe {
            asm!("rdcycle {x}", x = out(reg) x);
        }
        x
    }
}
