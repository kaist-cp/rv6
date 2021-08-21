// TODO: this file needs refactoring

use core::{fmt, mem};

use cortex_a::registers::*;
use tock_registers::interfaces::{Readable, Writeable};

use crate::{
    addr::PGSIZE,
    arch::interface::{MemLayout, TrapManager},
    arch::{
        asm::{intr_get, intr_off, intr_on, r_fpsr, w_fpsr},
        intr::INTERRUPT_CONTROLLER,
        memlayout::TIMER0_IRQ,
        proc::TrapFrame,
        ArmV8,
    },
    memlayout::{TRAMPOLINE, TRAPFRAME},
    trap::{IrqNum, IrqTypes, TrapTypes},
};

/// In ARM.v8 architecture, interrupts are part
/// of a more general term: exceptions.
enum ExceptionTypes {
    SyncException,
    IRQ,
    FIQ,
    SError,
}

impl ExceptionTypes {
    pub fn from_usize(n: usize) -> Self {
        match n {
            0 => Self::SyncException,
            1 => Self::IRQ,
            2 => Self::FIQ,
            3 => Self::SError,
            _ => panic!("invalud exception code"),
        }
    }
}

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];

    static mut userret: [u8; 0];
    fn vectors();
}

impl From<&IrqTypes> for IrqNum {
    fn from(item: &IrqTypes) -> Self {
        match item {
            IrqTypes::Uart => ArmV8::UART0_IRQ,
            IrqTypes::Virtio => ArmV8::VIRTIO0_IRQ,
            IrqTypes::Unknown(i) => *i,
            IrqTypes::Others(i) => *i,
        }
    }
}

impl TrapManager for ArmV8 {
    fn new() -> Self {
        Self {}
    }

    fn trap_init() {
        // nothing to do
    }

    /// Set up to take exceptions and traps while in the kernel.
    ///
    /// # Safety
    ///
    /// `vectors` must contain base address for a valid ARMv8-A exception vector table.
    unsafe fn trap_init_core() {
        VBAR_EL1.set(vectors as _);
    }

    fn get_trap_type(trap_info: usize) -> TrapTypes {
        let etype = ExceptionTypes::from_usize(trap_info);
        match etype {
            ExceptionTypes::SyncException => {
                if ESR_EL1.matches_all(ESR_EL1::EC::SVC64) {
                    TrapTypes::Syscall
                } else if ESR_EL1
                    .matches_any(ESR_EL1::EC::DataAbortLowerEL + ESR_EL1::EC::InstrAbortLowerEL)
                {
                    // TODO: Should handle these exceptions?
                    TrapTypes::BadTrap
                } else {
                    TrapTypes::BadTrap
                }
            }
            ExceptionTypes::IRQ => {
                let irq = INTERRUPT_CONTROLLER.fetch();

                let irq_type = match irq {
                    Some(i) => {
                        match i {
                            TIMER0_IRQ => {
                                return TrapTypes::TimerInterrupt;
                            }
                            ArmV8::UART0_IRQ => IrqTypes::Uart,
                            ArmV8::VIRTIO0_IRQ => IrqTypes::Virtio,
                            _ => IrqTypes::Unknown(i),
                        }
                    }
                    None => {
                        return TrapTypes::BadTrap;
                    }
                };

                TrapTypes::Irq(irq_type)
            }
            ExceptionTypes::FIQ | ExceptionTypes::SError => TrapTypes::BadTrap,
        }
    }

    fn is_user_trap() -> bool {
        SPSR_EL1.matches_all(SPSR_EL1::M::EL0t)
    }

    fn is_kernel_trap() -> bool {
        SPSR_EL1.matches_all(SPSR_EL1::M::EL1h) | SPSR_EL1.matches_all(SPSR_EL1::M::EL1t)
    }

    unsafe fn change_exception_vector(vector_table: usize) {
        VBAR_EL1.set(vector_table as u64);
    }

    /// do things before the kernel handle the trap.
    unsafe fn before_handling_trap(_trap: &TrapTypes, _trapframe: Option<&mut TrapFrame>) {}

    /// do things after the kernel handle the trap.
    unsafe fn after_handling_trap(trap: &TrapTypes) {
        match trap {
            TrapTypes::TimerInterrupt => {
                ArmV8::set_next_timer();
                INTERRUPT_CONTROLLER.finish(TIMER0_IRQ);
            }
            TrapTypes::Irq(irq_type) => {
                INTERRUPT_CONTROLLER.finish(irq_type.into());
            }
            _ => (),
        }
    }

    unsafe fn intr_on() {
        unsafe { intr_on() };
    }

    fn intr_off() {
        intr_off();
    }

    fn intr_get() -> bool {
        intr_get()
    }

    fn print_trap_status<F: Fn(fmt::Arguments<'_>)>(printer: F) {
        let elr_el1 = ELR_EL1.get();
        let spsr_el1 = SPSR_EL1.get();
        let far_el1 = FAR_EL1.get();
        let esr_el1 = ESR_EL1.get();

        printer(format_args!(
            "esr_el1: {:018p}\nspsr_el1={:018p} far_el1={:018p} elr_el1={:018p}",
            esr_el1 as *const u8, spsr_el1 as *const u8, far_el1 as *const u8, elr_el1 as *const u8
        ));
    }

    fn r_epc() -> usize {
        ELR_EL1.get() as usize
    }

    unsafe fn switch_to_kernel_vec() {
        unsafe {
            Self::change_exception_vector(vectors as _);
        }
    }

    unsafe fn switch_to_user_vec() {
        unsafe {
            Self::change_exception_vector(TRAMPOLINE);
        }
    }

    unsafe fn user_trap_ret(
        user_pagetable_addr: usize,
        trapframe: &mut TrapFrame,
        kernel_stack: usize,
        usertrap: usize,
    ) -> ! {
        // We're about to switch the destination of traps from
        // kerneltrap() to usertrap(), so turn off interrupts until
        // we're back in user space, where usertrap() is correct.
        intr_off();

        // Send syscalls, interrupts, and exceptions to trampoline.S.
        VBAR_EL1.set(TRAMPOLINE as u64);

        // kernel page table
        trapframe.kernel_satp = TTBR0_EL1.get() as usize;

        trapframe.kernel_trap = usertrap;

        trapframe.kernel_sp = kernel_stack + PGSIZE;

        // Tell trampoline.S the user page table to switch to.
        // Jump to trampoline.S at the top of memory, which
        // switches to the user page table, restores user registers,
        // and switches to user mode with sret.
        let fn_0: usize =
            TRAMPOLINE + unsafe { userret.as_ptr().offset_from(trampoline.as_ptr()) } as usize;
        let fn_0 = unsafe { mem::transmute::<_, unsafe extern "C" fn(usize, usize) -> !>(fn_0) };
        unsafe { fn_0(TRAPFRAME, user_pagetable_addr) }
    }

    fn save_trap_regs(store: &mut [usize; 10]) {
        let elr_el1 = ELR_EL1.get();
        let spsr_el1 = SPSR_EL1.get();
        let fpsr = r_fpsr();

        store[0] = elr_el1 as usize;
        store[1] = spsr_el1 as usize;
        store[2] = fpsr;
    }

    /// restore trap registers
    unsafe fn restore_trap_regs(store: &mut [usize; 10]) {
        let elr_el1 = store[0];
        let spsr_el1 = store[1];
        let fpsr = store[2];

        ELR_EL1.set(elr_el1 as u64);
        SPSR_EL1.set(spsr_el1 as u64);
        unsafe { w_fpsr(fpsr) };
    }
}
