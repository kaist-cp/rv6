use crate::libc;
extern "C" {
    #[link_name = "main"]
    fn main_0();
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    #[no_mangle]
    fn timervec();
    #[no_mangle]
    static mut stack0: [libc::c_char; 32768];
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
pub const CLINT_MTIME: libc::c_long =
    CLINT + 0xbff8 as libc::c_int as libc::c_long;
// which hart (core) is this?
#[inline]
unsafe extern "C" fn r_mhartid() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
    return x;
}
// Machine Status Register, mstatus
pub const MSTATUS_MPP_MASK: libc::c_long =
    (3 as libc::c_long) << 11 as libc::c_int;
// previous mode.
pub const MSTATUS_MPP_S: libc::c_long =
    (1 as libc::c_long) << 11 as libc::c_int;
pub const MSTATUS_MIE: libc::c_long = (1 as libc::c_long) << 3 as libc::c_int;
// machine-mode interrupt enable.
#[inline]
unsafe extern "C" fn r_mstatus() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
    return x;
}
#[inline]
unsafe extern "C" fn w_mstatus(mut x: uint64) {
    llvm_asm!("csrw mstatus, $0" : : "r" (x) : : "volatile");
}
// machine exception program counter, holds the
// instruction address to which a return from
// exception will go.
#[inline]
unsafe extern "C" fn w_mepc(mut x: uint64) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
}
// Machine-mode Interrupt Enable
// external
pub const MIE_MTIE: libc::c_long = (1 as libc::c_long) << 7 as libc::c_int;
// timer
// software
#[inline]
unsafe extern "C" fn r_mie() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
    return x;
}
#[inline]
unsafe extern "C" fn w_mie(mut x: uint64) {
    llvm_asm!("csrw mie, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_medeleg(mut x: uint64) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_mideleg(mut x: uint64) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}
// Machine-mode interrupt vector
#[inline]
unsafe extern "C" fn w_mtvec(mut x: uint64) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
}
// use riscv's sv39 page table scheme.
// supervisor address translation and protection;
// holds the address of the page table.
#[inline]
unsafe extern "C" fn w_satp(mut x: uint64) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_mscratch(mut x: uint64) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_tp(mut x: uint64) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}
// scratch area for timer interrupt, one per CPU.
#[no_mangle]
pub static mut mscratch0: [uint64; 256] = [0; 256];
// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe extern "C" fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let mut x: libc::c_ulong = r_mstatus();
    x &= !MSTATUS_MPP_MASK as libc::c_ulong;
    x |= MSTATUS_MPP_S as libc::c_ulong;
    w_mstatus(x);
    // set M Exception Program Counter to main, for mret.
  // requires gcc -mcmodel=medany
    w_mepc(::core::mem::transmute::<Option<unsafe extern "C" fn() -> ()>,
                                    uint64>(Some(::core::mem::transmute::<unsafe extern "C" fn()
                                                                              ->
                                                                                  (),
                                                                          unsafe extern "C" fn()
                                                                              ->
                                                                                  ()>(main_0))));
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
// set up to receive timer interrupts in machine mode,
// which arrive at timervec in kernelvec.S,
// which turns them into software interrupts for
// devintr() in trap.c.
#[no_mangle]
pub unsafe extern "C" fn timerinit() {
    // each CPU has a separate source of timer interrupts.
    let mut id: libc::c_int = r_mhartid() as libc::c_int;
    // ask the CLINT for a timer interrupt.
    let mut interval: libc::c_int =
        1000000 as libc::c_int; // cycles; about 1/10th second in qemu.
    *((CLINT + 0x4000 as libc::c_int as libc::c_long +
           (8 as libc::c_int * id) as libc::c_long) as *mut uint64) =
        (*(CLINT_MTIME as
               *mut uint64)).wrapping_add(interval as libc::c_ulong);
    // prepare information in scratch[] for timervec.
  // scratch[0..3] : space for timervec to save registers.
  // scratch[4] : address of CLINT MTIMECMP register.
  // scratch[5] : desired interval (in cycles) between timer interrupts.
    let mut scratch: *mut uint64 =
        &mut *mscratch0.as_mut_ptr().offset((32 as libc::c_int * id) as isize)
            as *mut uint64;
    *scratch.offset(4 as libc::c_int as isize) =
        (CLINT + 0x4000 as libc::c_int as libc::c_long +
             (8 as libc::c_int * id) as libc::c_long) as uint64;
    *scratch.offset(5 as libc::c_int as isize) = interval as uint64;
    w_mscratch(scratch as uint64);
    // set the machine-mode trap handler.
    w_mtvec(::core::mem::transmute::<Option<unsafe extern "C" fn() -> ()>,
                                     uint64>(Some(::core::mem::transmute::<unsafe extern "C" fn()
                                                                               ->
                                                                                   (),
                                                                           unsafe extern "C" fn()
                                                                               ->
                                                                                   ()>(timervec))));
    // enable machine-mode interrupts.
    w_mstatus(r_mstatus() | MSTATUS_MIE as libc::c_ulong);
    // enable machine-mode timer interrupts.
    w_mie(r_mie() | MIE_MTIE as libc::c_ulong);
}
