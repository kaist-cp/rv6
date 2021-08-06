use cortex_a::{asm::barrier, registers::*};
use tock_registers::interfaces::Writeable;

use crate::{
    arch::asm::*, arch::memlayout::MemLayoutImpl, kernel::main, memlayout::MemLayout, param::NCPU,
    uart::Uart,
};

extern "C" {
    // entry for all cores
    static mut _entry: [u8; 0];
}

/// entry.S needs one stack per CPU.
#[repr(C, align(16))]
pub struct Stack([[u8; 4096]; NCPU]);

impl Stack {
    const fn new() -> Self {
        Self([[0; 4096]; NCPU])
    }
}

#[no_mangle]
pub static mut stack0: Stack = Stack::new();

/// A scratch area per CPU for machine-mode timer interrupts.
static mut TIMER_SCRATCH: [[usize; NCPU]; 5] = [[0; NCPU]; 5];

/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe fn start() {
    let cur_el = r_currentel();

    match cur_el {
        0 => _puts("current el: 0\n"),
        1 => _puts("current el: 1\n"),
        2 => _puts("current el: 2\n"),
        3 => _puts("current el: 3\n"),
        _ => _puts("current el: unknown\n"),
    }

    if cpu_id() == 0 {
        // flush TLB and cache
        _puts("Flushing TLB and instr cache\n");

        // flush Instr Cache
        ic_ialluis();

        // flush TLB
        tlbi_vmalle1();
        unsafe { barrier::dsb(barrier::SY) };

        // no trapping on FP/SIMD instructions
        unsafe { w_cpacr_el1(0x03 << 20) };

        // monitor debug: all disabled
        unsafe { w_mdscr_el1(0) };

        // set_up_mair
        // TODO: This setting might be problematic.
        MAIR_EL1.write(
            // Attribute 1 - Cacheable normal DRAM.
            MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
            MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc +
            // Attribute 0 - Device.
            MAIR_EL1::Attr0_Device::nonGathering_nonReordering_EarlyWriteAck,
        );

        // set translation control register
        TCR_EL1.write(
            TCR_EL1::TBI1::Used
            + TCR_EL1::IPS::Bits_44 // intermediate physical address = 44bits
            + TCR_EL1::TG1::KiB_4 // transaltion granule = 4KB
            + TCR_EL1::TG0::KiB_4
            + TCR_EL1::SH0::Inner
            + TCR_EL1::SH1::Inner // Inner Shareable
            + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::EPD0::EnableTTBR0Walks
            + TCR_EL1::EPD1::EnableTTBR1Walks
            + TCR_EL1::A1::TTBR0 // use TTBR0_EL1's ASID as an ASID
            + TCR_EL1::T0SZ.val(25) // this can be changed, possible up to 44
            + TCR_EL1::T1SZ.val(25) // this can be changed, possible up to 44
            + TCR_EL1::AS::ASID16Bits // the upper 16 bits of TTBR0_EL1 and TTBR1_EL1 are used for allocation and matching in the TLB.
            + TCR_EL1::TBI0::Ignored, // this may not be needed
        );

        unsafe {
            launch_other_cores(_entry.as_mut_ptr() as usize);
        }
    }

    unsafe {
        main();
    }
}

fn _puts(s: &str) {
    for c in s.chars() {
        uart_putc(c as u8);
    }
}

fn uart_putc(c: u8) {
    let u_art = unsafe { Uart::new(MemLayoutImpl::UART0) };
    u_art.putc(c);
}

pub fn launch_other_cores(kernel_entry: usize) {
    let core_id = cpu_id();
    for i in 0..3 {
        if i != core_id {
            let _ = smc_call(SmcFunctions::CpuOn as u64, i as u64, kernel_entry as u64, 0);
            // crate::driver::psci::cpu_on(i, kernel_entry as u64, 0);
        }
    }
}
