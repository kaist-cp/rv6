use crate::libc;
use crate::{
    kernel_main::main_0,
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
pub struct Stack([libc::c_char; 4096 * NCPU as usize]);

impl Stack {
    const fn new() -> Self {
        Self([0; 4096 * NCPU as usize])
    }
}

#[no_mangle]
pub static mut stack0: Stack = Stack::new();

/// scratch area for timer interrupt, one per CPU.
static mut mscratch0: [u64; NCPU as usize * 32] = [0; NCPU as usize * 32];

/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let x = (r_mstatus() & !MSTATUS_MPP_MASK as u64) | MSTATUS_MPP_S as u64;
    w_mstatus(x);

    // set M Exception Program Counter to main, for mret.
    // requires gcc -mcmodel=medany
    w_mepc(main_0 as usize as u64);

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
    let mut id: i32 = r_mhartid() as i32;

    // ask the CLINT for a timer interrupt.
    let mut interval: i32 = 1000000; // cycles; about 1/10th second in qemu.
    *(clint_mtimecmp(id as u64) as *mut u64) =
        (*(CLINT_MTIME as *mut u64)).wrapping_add(interval as u64);

    // prepare information in scratch[] for timervec.
    // scratch[0..3] : space for timervec to save registers.
    // scratch[4] : address of CLINT MTIMECMP register.
    // scratch[5] : desired interval (in cycles) between timer interrupts.
    let mut scratch: *mut u64 = &mut *mscratch0.as_mut_ptr().offset(32 * id as isize) as *mut u64;
    *scratch.offset(4) = clint_mtimecmp(id as u64);
    *scratch.offset(5) = interval as u64;
    w_mscratch(scratch as u64);

    // set the machine-mode trap handler.
    w_mtvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        u64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(timervec))));

    // enable machine-mode interrupts.
    w_mstatus(r_mstatus() | MSTATUS_MIE as u64);

    // enable machine-mode timer interrupts.
    w_mie(r_mie() | MIE_MTIE as u64);
}
