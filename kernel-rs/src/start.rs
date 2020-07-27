use crate::kernel_main::main_0;
use crate::libc;
use crate::riscv::{
    r_mhartid, r_mie, r_mstatus, w_medeleg, w_mepc, w_mideleg, w_mie, w_mscratch, w_mstatus,
    w_mtvec, w_satp, w_tp, MIE_MTIE, MSTATUS_MIE, MSTATUS_MPP_MASK, MSTATUS_MPP_S,
};
extern "C" {
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    #[no_mangle]
    fn timervec();
}
pub type uint64 = libc::c_ulong;
// Physical memory layout
// qemu -machine virt is set up like this,
// based on qemu's hw/riscv/virt.c:
//
// 00001000 -- boot ROM, provided by qemu
// 02000000 -- CLINT
// 0C000000 -- PLIC
// 10000000 -- uart0
// 10001000 -- virtio disk
// 80000000 -- boot ROM jumps here in machine mode
//             -kernel loads the kernel here
// unused RAM after 80000000.
// the kernel uses physical memory thus:
// 80000000 -- entry.S, then kernel text and data
// end -- start of kernel page allocation area
// PHYSTOP -- end RAM used by the kernel
// qemu puts UART registers here in physical memory.
// virtio mmio interface
// local interrupt controller, which contains the timer.
pub const CLINT: libc::c_long = 0x2000000 as libc::c_long;
pub const CLINT_MTIME: libc::c_long = CLINT + 0xbff8 as libc::c_int as libc::c_long;
/// entry.S needs one stack per CPU.
#[repr(align(16))]
pub struct Stack([libc::c_char; 32768]);
#[no_mangle]
pub static mut stack0: Stack = Stack([0; 32768]);
// scratch area for timer interrupt, one per CPU.
#[no_mangle]
pub static mut mscratch0: [uint64; 256] = [0; 256];
/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe extern "C" fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let mut x: libc::c_ulong = r_mstatus();
    x &= !MSTATUS_MPP_MASK as libc::c_ulong;
    x |= MSTATUS_MPP_S as libc::c_ulong;
    w_mstatus(x);
    // set M Exception Program Counter to main, for mret.
    // requires gcc -mcmodel=medany
    w_mepc(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        uint64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(main_0))));
    // disable paging for now.
    w_satp(0 as libc::c_int as uint64);
    // delegate all interrupts and exceptions to supervisor mode.
    w_medeleg(0xffff as libc::c_int as uint64);
    w_mideleg(0xffff as libc::c_int as uint64);
    // ask for clock interrupts.
    timerinit();
    // keep each CPU's hartid in its tp register, for cpuid().
    let mut id: libc::c_int = r_mhartid() as libc::c_int;
    w_tp(id as uint64);
    // switch to supervisor mode and jump to main().
    llvm_asm!("mret" : : : : "volatile");
}
/// set up to receive timer interrupts in machine mode,
/// which arrive at timervec in kernelvec.S,
/// which turns them into software interrupts for
/// devintr() in trap.c.
#[no_mangle]
pub unsafe extern "C" fn timerinit() {
    // each CPU has a separate source of timer interrupts.
    let mut id: libc::c_int = r_mhartid() as libc::c_int;
    // ask the CLINT for a timer interrupt.
    let mut interval: libc::c_int = 1000000 as libc::c_int; // cycles; about 1/10th second in qemu.
    *((CLINT + 0x4000 as libc::c_int as libc::c_long + (8 as libc::c_int * id) as libc::c_long)
        as *mut uint64) = (*(CLINT_MTIME as *mut uint64)).wrapping_add(interval as libc::c_ulong);
    // prepare information in scratch[] for timervec.
    // scratch[0..3] : space for timervec to save registers.
    // scratch[4] : address of CLINT MTIMECMP register.
    // scratch[5] : desired interval (in cycles) between timer interrupts.
    let mut scratch: *mut uint64 = &mut *mscratch0
        .as_mut_ptr()
        .offset((32 as libc::c_int * id) as isize)
        as *mut uint64;
    *scratch.offset(4 as libc::c_int as isize) =
        (CLINT + 0x4000 as libc::c_int as libc::c_long + (8 as libc::c_int * id) as libc::c_long)
            as uint64;
    *scratch.offset(5 as libc::c_int as isize) = interval as uint64;
    w_mscratch(scratch as uint64);
    // set the machine-mode trap handler.
    w_mtvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        uint64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(timervec))));
    // enable machine-mode interrupts.
    w_mstatus(r_mstatus() | MSTATUS_MIE as libc::c_ulong);
    // enable machine-mode timer interrupts.
    w_mie(r_mie() | MIE_MTIE as libc::c_ulong);
}
