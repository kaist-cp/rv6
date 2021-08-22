use core::fmt;
use core::mem;

use crate::{
    addr::PGSIZE,
    arch::asm::{
        intr_get, intr_off, intr_on, make_satp, r_satp, r_scause, r_sepc, r_sip, r_stval, r_tp,
        w_sepc, w_sip, w_stvec, Sstatus,
    },
    arch::interface::{MemLayout, TrapManager},
    arch::intr::{plic_claim, plic_complete},
    arch::proc::RiscVTrapFrame as TrapFrame,
    arch::RiscV,
    memlayout::{TRAMPOLINE, TRAPFRAME},
    trap::{IrqNum, IrqTypes, TrapTypes},
};

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];

    static mut uservec: [u8; 0];

    static mut userret: [u8; 0];

    // In kernelvec.S, calls kerneltrap().
    fn kernelvec();
}

impl From<&IrqTypes> for IrqNum {
    fn from(item: &IrqTypes) -> Self {
        match item {
            IrqTypes::Uart => RiscV::UART0_IRQ,
            IrqTypes::Virtio => RiscV::VIRTIO0_IRQ,
            IrqTypes::Unknown(i) => *i,
            IrqTypes::Others(_) => 0,
        }
    }
}

impl TrapManager for RiscV {
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
        // Safety: `kernelvec` contains a valid trap vector.
        unsafe { w_stvec(kernelvec as _) };
    }

    fn get_trap_type(_trap_info: usize) -> TrapTypes {
        let scause = r_scause();
        if scause == 8 {
            TrapTypes::Syscall
        } else if scause & 0x8000000000000000 != 0 && scause & 0xff == 9 {
            // This is a supervisor external interrupt, via PLIC.

            // irq indicates which device interrupted.
            let irq = unsafe { plic_claim() } as usize;

            match irq {
                RiscV::UART0_IRQ => TrapTypes::Irq(IrqTypes::Uart),
                RiscV::VIRTIO0_IRQ => TrapTypes::Irq(IrqTypes::Virtio),
                0 => {
                    // TODO: should we handle this?
                    TrapTypes::Irq(IrqTypes::Others(0))
                }
                _ => TrapTypes::Irq(IrqTypes::Unknown(irq)),
            }
        } else if scause == 0x8000000000000001 {
            // Software interrupt from a machine-mode timer interrupt,
            // forwarded by timervec in selfvec.S.

            TrapTypes::TimerInterrupt
        } else {
            TrapTypes::BadTrap
        }
    }

    fn is_user_trap() -> bool {
        !Sstatus::read().contains(Sstatus::SPP)
    }

    fn is_kernel_trap() -> bool {
        let sstatus = Sstatus::read();
        sstatus.contains(Sstatus::SPP)
    }

    unsafe fn change_exception_vector(vector_table: usize) {
        unsafe {
            w_stvec(vector_table);
        }
    }

    /// do things before the kernel handle the trap.
    unsafe fn before_handling_trap(trap: &TrapTypes, trapframe: Option<&mut TrapFrame>) {
        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        if let TrapTypes::Syscall = trap {
            let trapframe = trapframe.expect("Syscall need trapframe!");
            trapframe.epc = (trapframe.epc).wrapping_add(4);
        }
    }

    /// do things after the kernel handle the trap.
    unsafe fn after_handling_trap(trap: &TrapTypes) {
        match trap {
            TrapTypes::Irq(irq_type) => {
                let irq_num: usize = irq_type.into();
                if irq_num != 0 {
                    unsafe {
                        plic_complete(irq_num as u32);
                    }
                }
            }
            TrapTypes::TimerInterrupt => {
                // Acknowledge the software interrupt by clearing
                // the SSIP bit in sip.
                unsafe { w_sip(r_sip() & !2) };
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
        let sepc = r_sepc();
        let scause = r_scause();
        let stval = r_stval();

        printer(format_args!(
            "scause {:018p}\nsepc={:018p} stval={:018p}\n",
            scause as *const u8, sepc as *const u8, stval as *const u8,
        ));
    }

    fn r_epc() -> usize {
        r_sepc()
    }

    unsafe fn switch_to_kernel_vec() {
        unsafe { w_stvec(kernelvec as _) };
    }

    unsafe fn switch_to_user_vec() {
        unsafe {
            w_stvec(
                TRAMPOLINE.wrapping_add(
                    uservec.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize
                ),
            )
        };
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
        // Safety: this points to a valid page table.
        unsafe {
            w_stvec(
                TRAMPOLINE.wrapping_add(
                    uservec.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize
                ),
            )
        };

        // Set up trapframe values that uservec will need when
        // the process next re-enters the kernel.

        // kernel page table
        trapframe.kernel_satp = r_satp();

        // process's kernel stack
        trapframe.kernel_sp = kernel_stack + PGSIZE;
        trapframe.kernel_trap = usertrap;

        // hartid for cpuid()
        trapframe.kernel_hartid = r_tp();

        // Set up the registers that trampoline.S's sret will use
        // to get to user space.

        // Set S Previous Privilege mode to User.
        let mut x = Sstatus::read();

        // Clear SPP to 0 for user mode.
        x.remove(Sstatus::SPP);

        // Enable interrupts in user mode.
        x.insert(Sstatus::SPIE);
        unsafe { x.write() };

        // Set S Exception Program Counter to the saved user pc.
        unsafe { w_sepc(trapframe.epc) };

        // Tell trampoline.S the user page table to switch to.
        let satp: usize = make_satp(user_pagetable_addr);

        // Jump to trampoline.S at the top of memory, which
        // switches to the user page table, restores user registers,
        // and switches to user mode with sret.
        let fn_0: usize =
            TRAMPOLINE + unsafe { userret.as_ptr().offset_from(trampoline.as_ptr()) } as usize;
        let fn_0 = unsafe { mem::transmute::<_, unsafe extern "C" fn(usize, usize) -> !>(fn_0) };
        unsafe { fn_0(TRAPFRAME, satp) }
    }

    /// Save trap registers in `store`.
    fn save_trap_regs(store: &mut [usize; 10]) {
        let sepc = r_sepc();
        let sstatus = Sstatus::read().bits();

        store[0] = sepc;
        store[1] = sstatus;
    }

    /// Restore trap registers from `store`.
    ///
    /// # Safety
    ///
    /// It must be matched with `save_trap_regs`, implying that `store` contains
    /// valid trap register values.
    unsafe fn restore_trap_regs(store: &mut [usize; 10]) {
        let sepc = store[0];
        let sstatus = store[1];

        unsafe {
            w_sepc(sepc);
            asm!("csrw sstatus, {}", in(reg) sstatus);
        }
    }
}
