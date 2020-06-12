use crate::libc;
extern "C" {
    // proc.c
    #[no_mangle]
    fn cpuid() -> libc::c_int;
}
pub type uint32 = libc::c_uint;
pub type uint64 = libc::c_ulong;
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
pub const UART0_IRQ: libc::c_int = 10 as libc::c_int;
// virtio mmio interface
pub const VIRTIO0_IRQ: libc::c_int = 1 as libc::c_int;
// local interrupt controller, which contains the timer.
// cycles since boot.
// qemu puts programmable interrupt controller here.
pub const PLIC: libc::c_long = 0xc000000 as libc::c_long;
pub const PLIC_PENDING: libc::c_long = PLIC + 0x1000 as libc::c_int as libc::c_long;
// plic.c
//
// the riscv Platform Level Interrupt Controller (PLIC).
//
#[no_mangle]
pub unsafe extern "C" fn plicinit() {
    // set desired IRQ priorities non-zero (otherwise disabled).
    *((PLIC + (UART0_IRQ * 4 as libc::c_int) as libc::c_long) as *mut uint32) =
        1 as libc::c_int as uint32;
    *((PLIC + (VIRTIO0_IRQ * 4 as libc::c_int) as libc::c_long) as *mut uint32) =
        1 as libc::c_int as uint32;
}
#[no_mangle]
pub unsafe extern "C" fn plicinithart() {
    let mut hart: libc::c_int = cpuid();
    // set uart's enable bit for this hart's S-mode.
    *((PLIC + 0x2080 as libc::c_int as libc::c_long + (hart * 0x100 as libc::c_int) as libc::c_long)
        as *mut uint32) =
        ((1 as libc::c_int) << UART0_IRQ | (1 as libc::c_int) << VIRTIO0_IRQ) as uint32;
    // set this hart's S-mode priority threshold to 0.
    *((PLIC
        + 0x201000 as libc::c_int as libc::c_long
        + (hart * 0x2000 as libc::c_int) as libc::c_long) as *mut uint32) =
        0 as libc::c_int as uint32;
}
// return a bitmap of which IRQs are waiting
// to be served.
#[no_mangle]
pub unsafe extern "C" fn plic_pending() -> uint64 {
    //mask = *(uint32*)(PLIC + 0x1000);
    //mask |= (uint64)*(uint32*)(PLIC + 0x1004) << 32;
    *(PLIC_PENDING as *mut uint64)
}
// ask the PLIC what interrupt we should serve.
#[no_mangle]
pub unsafe extern "C" fn plic_claim() -> libc::c_int {
    let mut hart: libc::c_int = cpuid();
    //int irq = *(uint32*)(PLIC + 0x201004);
    let mut irq: libc::c_int = *((PLIC
        + 0x201004 as libc::c_int as libc::c_long
        + (hart * 0x2000 as libc::c_int) as libc::c_long)
        as *mut uint32) as libc::c_int;
    irq
}
// tell the PLIC we've served this IRQ.
#[no_mangle]
pub unsafe extern "C" fn plic_complete(mut irq: libc::c_int) {
    let mut hart: libc::c_int = cpuid();
    //*(uint32*)(PLIC + 0x201004) = irq;
    *((PLIC
        + 0x201004 as libc::c_int as libc::c_long
        + (hart * 0x2000 as libc::c_int) as libc::c_long) as *mut uint32) = irq as uint32;
}
