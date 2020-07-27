extern "C" {
    #[link_name = "main"]
    fn main_0();
    // assembly code in kernelvec.S for machine-mode timer interrupt.
    #[no_mangle]
    fn timervec();
}
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
pub const CLINT: i64 = 0x2000000;
pub const CLINT_MTIME: i64 = CLINT + 0xbff8 as i32 as i64;
/// which hart (core) is this?
#[inline]
unsafe extern "C" fn r_mhartid() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
    x
}
// Machine Status Register, mstatus
pub const MSTATUS_MPP_MASK: i64 = (3 as i64) << 11 as i32;
// previous mode.
pub const MSTATUS_MPP_S: i64 = (1 as i64) << 11 as i32;
pub const MSTATUS_MIE: i64 = (1 as i64) << 3 as i32;
/// machine-mode interrupt enable.
#[inline]
unsafe extern "C" fn r_mstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_mstatus(mut x: u64) {
    llvm_asm!("csrw mstatus, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
unsafe extern "C" fn w_mepc(mut x: u64) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
}
// Machine-mode Interrupt Enable
// external
pub const MIE_MTIE: i64 = (1 as i64) << 7 as i32;
/// timer
/// software
#[inline]
unsafe extern "C" fn r_mie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_mie(mut x: u64) {
    llvm_asm!("csrw mie, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_medeleg(mut x: u64) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_mideleg(mut x: u64) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}
/// Machine-mode interrupt vector
#[inline]
unsafe extern "C" fn w_mtvec(mut x: u64) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
}
/// use riscv's sv39 page table scheme.
/// supervisor address translation and protection;
/// holds the address of the page table.
#[inline]
unsafe extern "C" fn w_satp(mut x: u64) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_mscratch(mut x: u64) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn w_tp(mut x: u64) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}
/// entry.S needs one stack per CPU.
#[repr(align(16))]
pub struct Stack([u8; 32768]);
#[no_mangle]
pub static mut stack0: Stack = Stack([0; 32768]);
// scratch area for timer interrupt, one per CPU.
#[no_mangle]
pub static mut mscratch0: [u64; 256] = [0; 256];
/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe extern "C" fn start() {
    // set M Previous Privilege mode to Supervisor, for mret.
    let mut x: u64 = r_mstatus();
    x &= !MSTATUS_MPP_MASK as u64;
    x |= MSTATUS_MPP_S as u64;
    w_mstatus(x);
    // set M Exception Program Counter to main, for mret.
    // requires gcc -mcmodel=medany
    w_mepc(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        u64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(main_0))));
    // disable paging for now.
    w_satp(0 as u64);
    // delegate all interrupts and exceptions to supervisor mode.
    w_medeleg(0xffff as u64);
    w_mideleg(0xffff as u64);
    // ask for clock interrupts.
    timerinit();
    // keep each CPU's hartid in its tp register, for cpuid().
    let mut id: i32 = r_mhartid() as i32;
    w_tp(id as u64);
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
    let mut id: i32 = r_mhartid() as i32;
    // ask the CLINT for a timer interrupt.
    let mut interval: i32 = 1000000; // cycles; about 1/10th second in qemu.
    *((CLINT + 0x4000 as i64 + (8 * id) as i64) as *mut u64) =
        (*(CLINT_MTIME as *mut u64)).wrapping_add(interval as u64);
    // prepare information in scratch[] for timervec.
    // scratch[0..3] : space for timervec to save registers.
    // scratch[4] : address of CLINT MTIMECMP register.
    // scratch[5] : desired interval (in cycles) between timer interrupts.
    let mut scratch: *mut u64 = &mut *mscratch0.as_mut_ptr().offset(32 * id as isize) as *mut u64;
    *scratch.offset(4) = (CLINT + 0x4000 as i64 + (8 * id) as i64) as u64;
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
