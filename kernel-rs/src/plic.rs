//! the riscv Platform Level Interrupt Controller (PLIC).
use crate::{
    memlayout::{plic_sclaim, plic_senable, plic_spriority, PLIC, UART0_IRQ, VIRTIO0_IRQ},
    proc::cpuid,
};

pub unsafe fn plicinit() {
    // set desired IRQ priorities non-zero (otherwise disabled).
    *((PLIC.wrapping_add(UART0_IRQ.wrapping_mul(4))) as *mut u32) = 1;
    *((PLIC + VIRTIO0_IRQ * 4) as *mut u32) = 1;
}

pub unsafe fn plicinithart() {
    let hart: usize = cpuid();

    // set uart's enable bit for this hart's S-mode.
    *(plic_senable(hart) as *mut u32) = (1 << UART0_IRQ | 1 << VIRTIO0_IRQ) as u32;

    // set this hart's S-mode priority threshold to 0.
    *(plic_spriority(hart) as *mut u32) = 0;
}

/// ask the PLIC what interrupt we should serve.
pub unsafe fn plic_claim() -> usize {
    let hart: usize = cpuid();
    let irq: usize = *(plic_sclaim(hart) as *mut usize);
    irq
}

/// tell the PLIC we've served this IRQ.
pub unsafe fn plic_complete(irq: usize) {
    let hart: usize = cpuid();
    *(plic_sclaim(hart) as *mut usize) = irq;
}
