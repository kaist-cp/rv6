use core::fmt;

use crate::arch::proc::TrapFrame;
use crate::trap::TrapTypes;

pub trait PageInitiator {
    /// Returns the list of addresses and range for devices that
    /// should be mapped physically in kernel page table.
    fn kernel_page_dev_mappings() -> &'static [(usize, usize)];

    unsafe fn switch_page_table_and_enable_mmu(page_table_base: usize);
}

pub trait MemLayout {
    /// qemu puts UART registers here in physical memory.
    const UART0: usize;

    /// virtio mmio interface
    const VIRTIO0: usize;

    /// the kernel expects there to be RAM
    /// for use by the kernel and user pages
    /// from physical address KERNBASE to PHYSTOP.
    const KERNBASE: usize;

    const UART0_IRQ: usize;
    const VIRTIO0_IRQ: usize;
}

pub trait TimeManager {
    fn timer_init();

    /// The uptime since power-on of the device, in microseconds.
    /// This includes time consumed by firmware and bootloaders.
    fn uptime_as_micro() -> Result<usize, ()>;
}

pub trait Arch: PageInitiator + MemLayout + TimeManager + TrapManager + PowerOff {}

pub trait TrapManager {
    fn new() -> Self;

    fn trap_init();

    unsafe fn trap_init_core();

    fn get_trap_type(arg: usize) -> TrapTypes;

    fn is_user_trap() -> bool;

    fn is_kernel_trap() -> bool;

    unsafe fn change_exception_vector(vector_table: usize);

    /// do things before the kernel handle the trap.
    unsafe fn before_handling_trap(trap: &TrapTypes, trapframe: Option<&mut TrapFrame>);

    /// do things after the kernel handle the trap.
    unsafe fn after_handling_trap(trap: &TrapTypes);

    unsafe fn intr_on();

    fn intr_off();

    fn intr_get() -> bool;

    fn print_trap_status<F: Fn(fmt::Arguments<'_>)>(printer: F);

    /// read pc at the moment trap occurs.
    fn r_epc() -> usize;

    unsafe fn switch_to_kernel_vec();

    unsafe fn switch_to_user_vec();

    unsafe fn user_trap_ret(
        user_pagetable_addr: usize,
        trap: &mut TrapFrame,
        kernel_stack: usize,
        usertrap: usize,
    ) -> !;

    fn save_trap_regs(store: &mut [usize; 10]);
    unsafe fn restore_trap_regs(store: &mut [usize; 10]);
}

pub trait PowerOff {
    /// Shutdowns this machine, discarding all unsaved data.
    fn machine_poweroff(_exitcode: u16) -> !;
}
