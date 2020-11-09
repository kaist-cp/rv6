use crate::{
    kernel::kernel_main,
    memlayout::{clint_mtimecmp, CLINT_MTIME},
    param::NCPU,
    riscv::{
        r_mhartid, w_medeleg, w_mepc, w_mideleg, w_mscratch, w_mtvec, w_satp, w_tp, Mstatus, MIE, SIE,
    },
};

extern "C" {
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    fn timervec();
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

/// scratch area for timer interrupt, one per CPU.
static mut MSCRATCH0: [[usize; 32]; NCPU] = [[0; 32]; NCPU];

/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let mut x = Mstatus::read();
    x.remove(Mstatus::MPP_MASK);
    x.insert(Mstatus::MPP_S);
    x.write();

    // set M Exception Program Counter to main, for mret.  requires gcc -mcmodel=medany
    w_mepc(kernel_main as usize);

    // disable paging for now.
    w_satp(0);

    // delegate all interrupts and exceptions to supervisor mode.
    w_medeleg(0xffff);
    w_mideleg(0xffff);
    let mut x = SIE::read();
    x.insert(SIE::SEIE);
    x.insert(SIE::STIE);
    x.insert(SIE::SSIE);
    x.write();

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
    let id = r_mhartid();

    // ask the CLINT for a timer interrupt.
    let interval: usize = 1_000_000; // cycles; about 1/10th second in qemu.
    *(clint_mtimecmp(id) as *mut usize) = (*(CLINT_MTIME as *mut usize)) + interval;

    // prepare information in scratch[] for timervec.
    // scratch[0..3] : space for timervec to save registers.
    // scratch[4] : address of CLINT MTIMECMP register.
    // scratch[5] : desired interval (in cycles) between timer interrupts.
    let scratch = &mut MSCRATCH0[id][..];
    *scratch.get_unchecked_mut(4) = clint_mtimecmp(id);
    *scratch.get_unchecked_mut(5) = interval;
    w_mscratch(&scratch[0] as *const _ as usize);

    // set the machine-mode trap handler.
    w_mtvec(timervec as _);

    // enable machine-mode interrupts.
    let mut x = Mstatus::read();
    x.insert(Mstatus::MIE);
    x.write();

    // enable machine-mode timer interrupts.
    let mut y = MIE::read();
    y.insert(MIE::MTIE);
    y.write();
}
