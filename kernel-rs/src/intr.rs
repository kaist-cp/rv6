use crate::{
    kernel::kernel_builder,
    riscv::{intr_get, intr_off, intr_on},
};

/// # Safety
///
/// * The current cpu must not be interruptible.
/// * The current cpu's `noff` equals the number of existing `HeldInterrupts`.
pub struct HeldInterrupts;

impl HeldInterrupts {
    pub fn new() -> Self {
        let old = intr_get();
        unsafe { intr_off() };

        let mut held = HeldInterrupts;

        // TODO: remove kernel_builder()
        let cpu = kernel_builder().current_cpu(&mut held);
        if cpu.noff == 0 {
            cpu.interrupt_enabled = old;
        }
        cpu.noff += 1;

        held
    }
}

impl Drop for HeldInterrupts {
    fn drop(&mut self) {
        debug_assert!(!intr_get(), "pop_off - interruptible");

        // TODO: remove kernel_builder()
        let cpu = kernel_builder().current_cpu(self);
        debug_assert!(cpu.noff >= 1, "pop_off");

        cpu.noff -= 1;

        if cpu.noff == 0 && cpu.interrupt_enabled {
            // SAFETY: no remaining HeldInterrupts; can enable interrupts.
            unsafe { intr_on() };
        }
    }
}
