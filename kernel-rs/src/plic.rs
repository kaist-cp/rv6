use crate::{
    memlayout::{
        plic_sclaim, plic_senable, plic_spriority, PLIC, PLIC_PENDING, UART0_IRQ, VIRTIO0_IRQ,
    },
    proc::cpuid,
};
/// local interrupt controller, which contains the timer.
/// cycles since boot.
///
/// the riscv Platform Level Interrupt Controller (PLIC).
///
pub unsafe fn plicinit() {
    // set desired IRQ priorities non-zero (otherwise disabled).
    *((PLIC + (UART0_IRQ * 4) as i64) as *mut u32) = 1 as i32 as u32;
    *((PLIC + (VIRTIO0_IRQ * 4) as i64) as *mut u32) = 1 as i32 as u32;
}
pub unsafe fn plicinithart() {
    let mut hart: i32 = cpuid();
    // set uart's enable bit for this hart's S-mode.
    *(plic_senable(hart) as *mut u32) =
        ((1 as i32) << UART0_IRQ | (1 as i32) << VIRTIO0_IRQ) as u32;
    // set this hart's S-mode priority threshold to 0.
    *(plic_spriority(hart) as *mut u32) = 0 as i32 as u32;
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
    let mut hart: i32 = cpuid();
    //int irq = *(u32*)(PLIC + 0x201004);
    let mut irq: i32 = *(plic_sclaim(hart) as *mut u32) as i32;
    irq
}
/// tell the PLIC we've served this IRQ.
pub unsafe fn plic_complete(mut irq: i32) {
    let mut hart: i32 = cpuid();
    //*(u32*)(PLIC + 0x201004) = irq;
    *(plic_sclaim(hart) as *mut u32) = irq as u32;
}
