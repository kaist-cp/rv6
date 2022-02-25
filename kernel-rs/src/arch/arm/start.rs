use cortex_a::{asm::barrier, registers::*};
use tock_registers::interfaces::{ReadWriteable, Writeable};

use crate::{
    arch::{
        asm::*,
        interface::{Arch, MemLayout, UartManagerConst},
        Armv8,
    },
    kernel::main,
    param::NCPU,
};

type Uart = <Armv8 as Arch>::Uart;

extern "C" {
    // entry for all cores
    static mut _entry: [u8; 0];
}

/// entry.S needs one stack per CPU.
#[derive(Debug)]
#[repr(C, align(16))]
pub struct Stack([[u8; 4096]; NCPU]);

impl Stack {
    const fn new() -> Self {
        Self([[0; 4096]; NCPU])
    }
}

#[no_mangle]
pub static mut stack0: Stack = Stack::new();

/// entry.S jumps here in machine mode on stack0.
///
/// # Safety
///
/// This function must be called from entry.S, and only once.
pub unsafe fn start() {
    // launch other cores
    if cpu_id() == 0 {
        // TODO(https://github.com/kaist-cp/rv6/issues/605): rustc bug?
        // when this fixed, change this line to below line
        let kernel_entry = 0x40010000;
        //let kernel_entry = unsafe { _entry.as_mut_ptr() as usize } as u64;
        for i in 1..3 {
            // SAFETY: Valid format for launching other CPU cores.
            let _ = unsafe { smc_call(SmcFunctions::CpuOn as u64, i, kernel_entry, 0) };
        }
    }

    let cur_el = r_currentel();

    // SAFETY: Assume that `Armv8::UART0` contains valid mapped address for uart.
    let uart = unsafe { Uart::new(Armv8::UART0) };

    uart.puts("current el: ");
    match cur_el {
        0 => uart.puts("0\n"),
        1 => uart.puts("1\n"),
        2 => uart.puts("2\n"),
        3 => uart.puts("3\n"),
        _ => uart.puts("unknown\n"),
    }

    // flush TLB and cache
    uart.puts("Flushing TLB and instr cache\n");

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
    // TODO: If device memory is needed for some attribute,
    // use MEM_ATTR_IDX_N on arch/arm/vm.rs for set_entry function
    MAIR_EL1.write(
        // Attribute 0, 1 - Cacheable normal DRAM.
        MAIR_EL1::Attr0_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc
            + MAIR_EL1::Attr0_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
            + MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc
            + MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc,
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

    SCTLR_EL1.modify(SCTLR_EL1::SA0::CLEAR);
    barrier();

    unsafe {
        main();
    }
}
