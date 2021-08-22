use core::fmt;

use crate::arch::TargetArch;
use crate::proc::RegNum;
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

pub trait Arch:
    PageInitiator + MemLayout + TimeManager + TrapManager + InterruptManager + ProcManager + PowerOff
{
    /// Which hart (core) is this?
    fn cpu_id() -> usize;
}

pub trait TrapManager {
    fn new() -> Self;

    /// Do some trap initialization needed only once.
    /// It is only called by boot core once.
    fn trap_init();

    /// Do some trap initialization needed for each core.
    ///
    /// # Safety
    ///
    /// Must be called only once for each core.
    unsafe fn trap_init_core();

    /// Get the type of invoked trap.
    fn get_trap_type(arg: usize) -> TrapTypes;

    fn is_user_trap() -> bool;

    fn is_kernel_trap() -> bool;

    /// Change exception vector to `vector_table`.
    ///
    /// # Safety
    ///
    /// `vector_table` must be a valid vector table.
    unsafe fn change_exception_vector(vector_table: usize);

    /// Do things before the kernel handle the trap.
    ///
    /// # Safety
    ///
    /// * Received trap type must have been actually occured.
    /// * Must be called before kernel handles `trap`.
    unsafe fn before_handling_trap(
        trap: &TrapTypes,
        trapframe: Option<&mut <TargetArch as ProcManager>::TrapFrame>,
    );

    /// Do things After the kernel handle the trap.
    ///
    /// # Safety
    ///
    /// * Received trap type must have been actually occured.
    /// * Must be called after kernel handles `trap`.
    unsafe fn after_handling_trap(trap: &TrapTypes);

    /// Turn the interrupt on.
    ///
    /// # Safety
    ///
    /// Interrupt handler must have been configured properly in advance.
    unsafe fn intr_on();

    fn intr_off();

    fn intr_get() -> bool;

    fn print_trap_status<F: Fn(fmt::Arguments<'_>)>(printer: F);

    /// read pc at the moment trap occurs.
    fn r_epc() -> usize;

    /// Switch the kernel vector to one for kernel.
    ///
    /// # Safety
    ///
    /// Interrupt handler must have been configured properly.
    unsafe fn switch_to_kernel_vec();

    /// Switch the kernel vector to one for user.
    ///
    /// # Safety
    ///
    /// Interrupt handler must have been configured properly.
    unsafe fn switch_to_user_vec();

    /// Go back to the user space after handling user trap.
    ///
    /// # Safety
    ///
    /// Must be called by `user_trap`, after handling the trap.
    unsafe fn user_trap_ret(
        user_pagetable_addr: usize,
        trap: &mut <TargetArch as ProcManager>::TrapFrame,
        kernel_stack: usize,
        usertrap: usize,
    ) -> !;

    fn save_trap_regs(store: &mut [usize; 10]);

    /// Restore trap registers from `store`.
    ///
    /// # Safety
    ///
    /// It must be matched with `save_trap_regs`, implying that `store` contains
    /// valid trap register values.
    unsafe fn restore_trap_regs(store: &mut [usize; 10]);
}

pub trait PowerOff {
    /// Shutdowns this machine, discarding all unsaved data.
    fn machine_poweroff(_exitcode: u16) -> !;
}

pub trait InterruptManager {
    unsafe fn intr_init();

    unsafe fn intr_init_core();
}

pub trait ProcManager {
    type TrapFrame: TrapFrameManager;
    type Context: ContextManager;

    /// Get binary of the user program that calls exec("/init").
    /// od -t xC initcode
    fn get_init_code() -> &'static [u8];
}

pub trait TrapFrameManager: Copy + Clone {
    fn set_pc(&mut self, val: usize);

    /// Set the value of return value register
    fn set_ret_val(&mut self, val: usize);

    /// Set the value of function argument register
    fn param_reg_mut(&mut self, index: RegNum) -> &mut usize;

    /// Get the value of function argument register
    fn get_param_reg(&self, index: RegNum) -> usize;

    fn init_reg(&mut self);
}

pub trait ContextManager: Copy + Clone + Default {
    fn new() -> Self;

    /// Set return register (lr)
    fn set_ret_addr(&mut self, val: usize);
}

// pub trait UserProcInitiator {
//     /// Initialize regiters for running first user process.
//     fn init_reg(trap_frame: &mut TrapFrame);
// }
