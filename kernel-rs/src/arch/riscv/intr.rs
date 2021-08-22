//! the riscv Platform Level Interrupt Controller (PLIC).
use crate::arch::{
    asm::r_tp,
    interface::InterruptManager,
    interface::MemLayout,
    memlayout::{plic_sclaim, plic_senable, plic_spriority, PLIC},
    RiscV,
};

impl InterruptManager for RiscV {
    unsafe fn intr_init() {
        // set desired IRQ priorities non-zero (otherwise disabled).
        unsafe { *((PLIC.wrapping_add(RiscV::UART0_IRQ.wrapping_mul(4))) as *mut u32) = 1 };
        unsafe { *((PLIC + RiscV::VIRTIO0_IRQ * 4) as *mut u32) = 1 };
    }

    unsafe fn intr_init_core() {
        let hart: usize = r_tp();

        // set uart's enable bit for this hart's S-mode.
        unsafe {
            *(plic_senable(hart) as *mut u32) =
                (1 << RiscV::UART0_IRQ | 1 << RiscV::VIRTIO0_IRQ) as u32
        };

        // set this hart's S-mode priority threshold to 0.
        unsafe { *(plic_spriority(hart) as *mut u32) = 0 };
    }
}

/// ask the PLIC what interrupt we should serve.
pub unsafe fn plic_claim() -> u32 {
    let hart: usize = r_tp();
    let irq: u32 = unsafe { *(plic_sclaim(hart) as *mut u32) };
    irq
}

/// tell the PLIC we've served this IRQ.
pub unsafe fn plic_complete(irq: u32) {
    let hart: usize = r_tp();
    unsafe { *(plic_sclaim(hart) as *mut u32) = irq };
}
