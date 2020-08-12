//! the riscv Platform Level Interrupt Controller (PLIC).
use crate::{
    memlayout::{
        plic_sclaim, plic_senable, plic_spriority, PLIC, PLIC_PENDING, UART0_IRQ, VIRTIO0_IRQ,
    },
    proc::cpuid,
};

pub unsafe fn plicinit() {
    // set desired IRQ priorities non-zero (otherwise disabled).
    *((PLIC as i64 + (UART0_IRQ * 4) as i64) as *mut u32) = 1;
    *((PLIC as i64 + (VIRTIO0_IRQ * 4) as i64) as *mut u32) = 1;
}

pub unsafe fn plicinithart() {
    let hart: i32 = cpuid();

    // set uart's enable bit for this hart's S-mode.
    *(plic_senable(hart) as *mut u32) = (1 << UART0_IRQ | 1 << VIRTIO0_IRQ) as u32;

    // set this hart's S-mode priority threshold to 0.
    *(plic_spriority(hart) as *mut u32) = 0;
}

/// return a bitmap of which IRQs are waiting
/// to be served.
pub unsafe fn plic_pending() -> u32 {
    //mask = *(u32*)(PLIC + 0x1000);
    //mask |= (u32)*(u32*)(PLIC + 0x1004) << 32;
    *(PLIC_PENDING as *mut u32)
}

/// ask the PLIC what interrupt we should serve.
pub unsafe fn plic_claim() -> i32 {
    let hart: i32 = cpuid();
    //int irq = *(u32*)(PLIC + 0x201004);
    let irq: i32 = *(plic_sclaim(hart) as *mut u32) as i32;
    irq
}

/// tell the PLIC we've served this IRQ.
pub unsafe fn plic_complete(irq: i32) {
    let hart: i32 = cpuid();
    //*(u32*)(PLIC + 0x201004) = irq;
    *(plic_sclaim(hart) as *mut u32) = irq as u32;
}
