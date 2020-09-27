use core::ptr;
use crate::memlayout;

/// Shutdowns this machine, discarding all unsaved data.
///
/// This function uses SiFive Test Finalizer, which provides power management for QEMU virt device.
pub fn machine_poweroff(exitcode: u16) -> ! {
    const BASE_CODE: u64 = 0x3333;
    let code = ((exitcode as u64) << 16) | BASE_CODE;
    // SAFETY:
    // - FINISHER is identically mapped from physical address.
    // - FINISHER is for MMIO. Though this is not specified as document, see the implementation:
    // https://github.com/qemu/qemu/blob/stable-5.0/hw/riscv/virt.c#L60 and,
    // https://github.com/qemu/qemu/blob/stable-5.0/hw/riscv/sifive_test.c#L34
    unsafe {
        ptr::write_volatile(memlayout::FINISHER as *mut u64, code);
    }

    unreachable!("Power off failed");
}
