use crate::{
    kernel_main::kernel_main,
    memlayout::{clint_mtimecmp, CLINT_MTIME},
    param::NCPU,
    riscv::{
        r_mhartid, r_mie, r_mstatus, w_medeleg, w_mepc, w_mideleg, w_mie, w_mscratch, w_mstatus,
        w_mtvec, w_satp, w_tp, MIE_MTIE, MSTATUS_MIE, MSTATUS_MPP_MASK, MSTATUS_MPP_S,
    },
};

extern "C" {
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    #[no_mangle]
    fn timervec();
}

/// entry.S needs one stack per CPU.
#[repr(align(16))]
pub struct Stack([u8; 4096 * NCPU as usize]);

impl Stack {
    const fn new() -> Self {
        Self([0; NCPU.wrapping_mul(4096)])
    }
}

#[no_mangle]
pub static mut stack0: Stack = Stack::new();

/// scratch area for timer interrupt, one per CPU.
static mut MSCRATCH0: [usize; NCPU.wrapping_mul(32)] = [0; NCPU.wrapping_mul(32)];

/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let x = (r_mstatus() & !MSTATUS_MPP_MASK) | MSTATUS_MPP_S;
    w_mstatus(x);

    // set M Exception Program Counter to main, for mret.
    // requires gcc -mcmodel=medany
    w_mepc(kernel_main as usize);

    // disable paging for now.
    w_satp(0);

    // delegate all interrupts and exceptions to supervisor mode.
    w_medeleg(0xffff);
    w_mideleg(0xffff);

    // ask for clock interrupts.
    timerinit();

    // keep each CPU's hartid in its tp register, for cpuid().
    w_tp(r_mhartid());

    // switch to supervisor mode and jump to main().
    llvm_asm!("mret" : : : : "volatile");
}

/// set up to receive timer interrupts in machine mode,
/// which arrive at timervec in kernelvec.S,
/// which turns them into software interrupts for devintr() in trap.c.
unsafe fn timerinit() {
    // each CPU has a separate source of timer interrupts.
    let id: i32 = r_mhartid() as i32;

    // ask the CLINT for a timer interrupt.

    // cycles; about 1/10th second in qemu.
    let interval: usize = 1000000;
    *(clint_mtimecmp(id as usize) as *mut usize) =
        (*(CLINT_MTIME as *mut usize)).wrapping_add(interval);

    // prepare information in scratch[] for timervec.
    // scratch[0..3] : space for timervec to save registers.
    // scratch[4] : address of CLINT MTIMECMP register.
    // scratch[5] : desired interval (in cycles) between timer interrupts.
    let scratch: *mut usize = &mut *MSCRATCH0.as_mut_ptr().offset(32 * id as isize) as *mut usize;
    *scratch.offset(4) = clint_mtimecmp(id as usize);
    *scratch.offset(5) = interval;
    w_mscratch(scratch as usize);

    // set the machine-mode trap handler.
    w_mtvec(timervec as _);

    // enable machine-mode interrupts.
    w_mstatus(r_mstatus() | MSTATUS_MIE);

    // enable machine-mode timer interrupts.
    w_mie(r_mie() | MIE_MTIE as usize);
}
