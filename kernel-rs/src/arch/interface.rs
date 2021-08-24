use core::fmt;

use crate::{
    addr::{Addr, PAddr},
    arch::TargetArch,
    proc::RegNum,
    trap::TrapTypes,
    vm::{AccessFlags, RawPageTable},
};

// TODO: Is this abstraction appropriate?

pub trait Arch:
    PageTableManager + MemLayout + TimeManager + TrapManager + InterruptManager + ProcManager + PowerOff
{
    type Uart: UartManager;

    /// # Safety
    ///
    /// This function must be called from entry.S, and only once.
    unsafe fn start();
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

    fn r_cycle() -> usize;
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
    /// Corresponding interrupt handler must have been configured properly.
    unsafe fn switch_to_kernel_vec();

    /// Switch the kernel vector to one for user.
    ///
    /// # Safety
    ///
    /// Must be called just before going back to user mode,
    /// after handling an invoked user trap.
    unsafe fn switch_to_user_vec();

    /// Go back to the user space after handling user trap.
    ///
    /// # Safety
    ///
    /// Must be called by `user_trap_ret`, after handling the user trap.
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
    /// Initialize device interrupt controller (globally).
    ///
    /// # Safety
    ///
    /// * Must be called only once across all the cores.
    /// * Must be called before any interrupt occurs.
    unsafe fn intr_init();

    /// Initialize device interrupt controller for each core.
    ///
    /// # Safety
    ///
    /// * Must be called only once for each core.
    /// * Must be called before any interrupt occurs.
    unsafe fn intr_init_core();
}

pub trait ProcManager {
    type TrapFrame: TrapFrameManager;
    type Context: ContextManager;

    /// Get binary of the user program that calls exec("/init").
    /// od -t xC initcode
    fn get_init_code() -> &'static [u8];

    /// Which hart (core) is this?
    fn cpu_id() -> usize;
}

pub trait TrapFrameManager: Copy + Clone {
    /// Set user pc.
    fn set_pc(&mut self, val: usize);

    /// Set the value of user stack pointer.
    fn set_sp(&mut self, val: usize);

    /// Set the value of return value register.
    fn set_ret_val(&mut self, val: usize);

    /// Set the value of function argument register.
    fn param_reg_mut(&mut self, index: RegNum) -> &mut usize;

    /// Get the value of function argument register.
    fn get_param_reg(&self, index: RegNum) -> usize;

    /// Initialize arch-specific registers.
    fn init_reg(&mut self);
}

pub trait ContextManager: Copy + Clone + Default {
    fn new() -> Self;

    /// Set return register (lr)
    fn set_ret_addr(&mut self, val: usize);
}

pub trait PageTableManager {
    type PageTableEntry: IPageTableEntry;

    /// The number of page table levels.
    const PLNUM: usize;

    /// Returns the list of addresses and range for devices that
    /// should be mapped physically in kernel page table.
    fn kernel_page_dev_mappings() -> &'static [(usize, usize)];

    /// Switch h/w page table register to the kernel's page table, and enable paging.
    ///
    /// # Safety
    ///
    /// `page_table_base` must contain base address for a valid page table, containing mapping for current pc.
    unsafe fn switch_page_table_and_enable_mmu(page_table_base: usize);
}

/// # Safety
///
/// If self.is_table() is true, then it must refer to a valid page-table page.
///
/// inner value should be initially 0, which satisfies the invariant.
pub trait IPageTableEntry: Default {
    type EntryFlags: From<AccessFlags>;

    fn get_flags(&self) -> Self::EntryFlags;

    fn flag_intersects(&self, flag: Self::EntryFlags) -> bool;

    fn get_pa(&self) -> PAddr;

    fn is_valid(&self) -> bool;

    fn is_user(&self) -> bool;

    fn is_table(&self) -> bool;

    fn is_data(&self) -> bool;

    /// Make the entry refer to a given page-table page.
    fn set_table(&mut self, page: *mut RawPageTable);

    /// Make the entry refer to a given address with a given permission.
    /// The permission should include at lease one of R, W, and X not to be
    /// considered as an entry referring a page-table page.
    fn set_entry(&mut self, pa: PAddr, perm: Self::EntryFlags);

    /// Make the entry inaccessible by user processes by clearing PteFlags::U.
    fn clear_user(&mut self);

    /// Invalidate the entry by making every bit 0.
    fn invalidate(&mut self);

    /// Return `Some(..)` if it refers to a page-table page.
    /// Return `None` if it refers to a data page.
    /// Return `None` if it is invalid.
    fn as_table_mut(&mut self) -> Option<&mut RawPageTable> {
        if self.is_table() {
            // SAFETY: invariant.
            Some(unsafe { &mut *(self.get_pa().into_usize() as *mut _) })
        } else {
            None
        }
    }
}

pub trait UartManagerConst {
    /// Create new Uart manager with base addreess `uart`.
    ///
    /// # Safety
    ///
    /// `uart` must be a valid mapped address.
    unsafe fn new(uart: usize) -> Self;
}

pub trait UartManager: UartManagerConst {
    fn init(&self);

    /// Read one input character from the UART. Return Err(()) if none is waiting.
    fn getc(&self) -> Result<i32, ()>;

    /// Write one output character to the UART.
    fn putc(&self, c: u8);

    /// Check whether the UART transmit holding register is full.
    fn is_full(&self) -> bool;
}
