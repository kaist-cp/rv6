use core::arch::asm;

use crate::{
    arch::asm::{
        r_mhartid, w_medeleg, w_mepc, w_mideleg, w_mscratch, w_mtvec, w_satp, w_tp, Mstatus, MIE,
        SIE,
    },
    arch::memlayout::{clint_mtimecmp, CLINT_MTIME},
    kernel::main,
    param::NCPU,
};

extern "C" {
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    fn timervec();
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

/// A scratch area per CPU for machine-mode timer interrupts.
static mut TIMER_SCRATCH: [[usize; NCPU]; 5] = [[0; NCPU]; 5];

/// entry.S jumps here in machine mode on stack0.
pub unsafe fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let mut x = Mstatus::read();
    x.remove(Mstatus::MPP_MASK);
    x.insert(Mstatus::MPP_S);
    unsafe { x.write() };

    // set M Exception Program Counter to main, for mret.  requires gcc -mcmodel=medany
    unsafe { w_mepc(main as usize) };

    // disable paging for now.
    unsafe { w_satp(0) };

    // delegate all interrupts and exceptions to supervisor mode.
    unsafe { w_medeleg(0xffff) };
    unsafe { w_mideleg(0xffff) };
    let mut x = SIE::read();
    x.insert(SIE::SEIE);
    x.insert(SIE::STIE);
    x.insert(SIE::SSIE);
    unsafe { x.write() };

    // ask for clock interrupts.
    unsafe { timerinit() };

    // keep each CPU's hartid in its tp register, for cpuid().
    unsafe { w_tp(r_mhartid()) };

    unsafe {
        // switch to supervisor mode and jump to main().
        asm!("mret");
    }
}

/// set up to receive timer interrupts in machine mode,
/// which arrive at timervec in kernelvec.S,
/// which turns them into software interrupts for devintr() in trap.c.
unsafe fn timerinit() {
    // each CPU has a separate source of timer interrupts.
    let id = r_mhartid();

    // ask the CLINT for a timer interrupt.
    let interval: usize = 1_000_000; // cycles; about 1/10th second in qemu.
    unsafe { *(clint_mtimecmp(id) as *mut usize) = (*(CLINT_MTIME as *mut usize)) + interval };

    // prepare information in scratch[] for timervec.
    // scratch[0..2] : space for timervec to save registers.
    // scratch[3] : address of CLINT MTIMECMP register.
    // scratch[4] : desired interval (in cycles) between timer interrupts.
    let scratch = unsafe { &mut TIMER_SCRATCH[id][..] };
    *unsafe { scratch.get_unchecked_mut(3) } = clint_mtimecmp(id);
    *unsafe { scratch.get_unchecked_mut(4) } = interval;
    unsafe { w_mscratch(&scratch[0] as *const _ as usize) };

    // set the machine-mode trap handler.
    unsafe { w_mtvec(timervec as _) };

    // enable machine-mode interrupts.
    let mut x = Mstatus::read();
    x.insert(Mstatus::MIE);
    unsafe { x.write() };

    // enable machine-mode timer interrupts.
    let mut y = MIE::read();
    y.insert(MIE::MTIE);
    unsafe { y.write() };
}
