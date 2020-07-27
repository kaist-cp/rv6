extern "C" {
    // proc.c
    #[no_mangle]
    fn cpuid() -> i32;
}
// Physical memory layout
// qemu -machine virt is set up like this,
// based on qemu's hw/riscv/virt.c:
//
// 00001000 -- boot ROM, provided by qemu
// 02000000 -- CLINT
// 0C000000 -- PLIC
// 10000000 -- uart0
// 10001000 -- virtio disk
// 80000000 -- boot ROM jumps here in machine mode
//             -kernel loads the kernel here
// unused RAM after 80000000.
// the kernel uses physical memory thus:
// 80000000 -- entry.S, then kernel text and data
// end -- start of kernel page allocation area
// PHYSTOP -- end RAM used by the kernel
// qemu puts UART registers here in physical memory.
pub const UART0_IRQ: i32 = 10;
// virtio mmio interface
pub const VIRTIO0_IRQ: i32 = 1;
// local interrupt controller, which contains the timer.
// cycles since boot.
// qemu puts programmable interrupt controller here.
pub const PLIC: i64 = 0xc000000;
pub const PLIC_PENDING: i64 = PLIC + 0x1000;
// plic.c
///
/// the riscv Platform Level Interrupt Controller (PLIC).
///
#[no_mangle]
pub unsafe extern "C" fn plicinit() {
    // set desired IRQ priorities non-zero (otherwise disabled).
    *((PLIC + (UART0_IRQ * 4) as i64) as *mut u32) = 1 as i32 as u32;
    *((PLIC + (VIRTIO0_IRQ * 4) as i64) as *mut u32) = 1 as i32 as u32;
}
#[no_mangle]
pub unsafe extern "C" fn plicinithart() {
    let mut hart: i32 = cpuid();
    // set uart's enable bit for this hart's S-mode.
    *((PLIC + 0x2080 as i32 as i64 + (hart * 0x100 as i32) as i64) as *mut u32) =
        ((1 as i32) << UART0_IRQ | (1 as i32) << VIRTIO0_IRQ) as u32;
    // set this hart's S-mode priority threshold to 0.
    *((PLIC + 0x201000 as i32 as i64 + (hart * 0x2000) as i64) as *mut u32) = 0 as i32 as u32;
}
/// return a bitmap of which IRQs are waiting
/// to be served.
#[no_mangle]
pub unsafe extern "C" fn plic_pending() -> u32 {
    //mask = *(u32*)(PLIC + 0x1000);
    //mask |= (u32)*(u32*)(PLIC + 0x1004) << 32;
    *(PLIC_PENDING as *mut u32)
}
/// ask the PLIC what interrupt we should serve.
#[no_mangle]
pub unsafe extern "C" fn plic_claim() -> i32 {
    let mut hart: i32 = cpuid();
    //int irq = *(u32*)(PLIC + 0x201004);
    let mut irq: i32 = *((PLIC + 0x201004 as i64 + (hart * 0x2000) as i64) as *mut u32) as i32;
    irq
}
/// tell the PLIC we've served this IRQ.
#[no_mangle]
pub unsafe extern "C" fn plic_complete(mut irq: i32) {
    let mut hart: i32 = cpuid();
    //*(u32*)(PLIC + 0x201004) = irq;
    *((PLIC + 0x201004 as i64 + (hart * 0x2000) as i64) as *mut u32) = irq as u32;
}
