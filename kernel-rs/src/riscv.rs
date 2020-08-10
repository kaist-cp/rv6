/// which hart (core) is this?
#[inline]
pub unsafe fn r_mhartid() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
    x
}

/// Machine Status Register, mstatus

/// previous mode.
pub const MSTATUS_MPP_MASK: i64 = (3) << 11;
pub const MSTATUS_MPP_M: i64 = (3) << 11;
pub const MSTATUS_MPP_S: i64 = (1) << 11;
pub const MSTATUS_MPP_U: i64 = (0) << 11;
/// machine-mode interrupt enable.
pub const MSTATUS_MIE: i64 = (1) << 3;

#[inline]
pub unsafe fn r_mstatus() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_mstatus(mut x: usize) {
    llvm_asm!("csrw mstatus, $0" : : "r" (x) : : "volatile");
}

/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_mepc(mut x: usize) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Status Register, sstatus

/// Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPP: i64 = (1) << 8;

/// Supervisor Previous Interrupt Enable
pub const SSTATUS_SPIE: i64 = (1) << 5;

/// User Previous Interrupt Enable
pub const SSTATUS_UPIE: i64 = (1) << 4;

/// Supervisor Interrupt Enable
pub const SSTATUS_SIE: i64 = (1) << 1;

/// User Interrupt Enable
pub const SSTATUS_UIE: i64 = (1) << 0;

#[inline]
pub unsafe fn r_sstatus() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sstatus(mut x: usize) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Interrupt Pending
#[inline]
pub unsafe fn r_sip() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sip(mut x: usize) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Interrupt Enable

/// external
pub const SIE_SEIE: i64 = (1) << 9;

/// timer
pub const SIE_STIE: i64 = (1) << 5;

/// software
pub const SIE_SSIE: i64 = (1) << 1;

#[inline]
pub unsafe fn r_sie() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_sie(mut x: usize) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}

/// Machine-mode Interrupt Enable

/// external
pub const MIE_MEIE: i64 = (1) << 11;

/// timer
pub const MIE_MTIE: i64 = (1) << 7;

/// software
pub const MIE_MSIE: i64 = (1) << 3;
#[inline]
pub unsafe fn r_mie() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_mie(mut x: usize) {
    llvm_asm!("csrw mie, $0" : : "r" (x) : : "volatile");
}

/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_sepc(mut x: usize) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_sepc() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}

/// Machine Exception Delegation
#[inline]
pub unsafe fn r_medeleg() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr %0, medeleg" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_medeleg(mut x: usize) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}

/// Machine Interrupt Delegation
#[inline]
pub unsafe fn r_mideleg() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr %0, mideleg" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_mideleg(mut x: usize) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe fn w_stvec(mut x: usize) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_stvec() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr %0, stvec" : "=r" (x) : : : "volatile");
    x
}

/// Machine-mode interrupt vector
#[inline]
pub unsafe fn w_mtvec(mut x: usize) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
}

/// use riscv's sv39 page table scheme.
pub const SATP_SV39: i64 = (8) << 60;

pub const fn make_satp(pagetable: usize) -> usize {
    SATP_SV39 as usize | pagetable >> 12
}

/// supervisor address translation and protection;
/// holds the address of the page table.
#[inline]
pub unsafe fn w_satp(mut x: usize) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_satp() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}

/// Supervisor Scratch register, for early trap handler in trampoline.S.
#[inline]
pub unsafe fn w_sscratch(mut x: usize) {
    llvm_asm!("csrw sscratch, %0" : : "r" (x) : : : "volatile");
}

#[inline]
pub unsafe fn w_mscratch(mut x: usize) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Trap Cause
#[inline]
pub unsafe fn r_scause() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}

/// Supervisor Trap Value
#[inline]
pub unsafe fn r_stval() -> usize {
    let mut x: usize = 0;
    llvm_asm!("csrr $0, stval" : "=r" (x) : : : "volatile");
    x
}

/// Machine-mode Counter-Enable
#[inline]
pub unsafe fn w_mcounteren(mut x: u64) {
    llvm_asm!("csrw mcounteren, %0" : : "r" (x)  : : : "volatile");
}

#[inline]
pub unsafe fn r_mcounteren() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr %0, mcounteren" : "=r" (x) : : : "volatile");
    x
}

/// machine-mode cycle counter
#[inline]
pub unsafe fn r_time() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr %0, time" : "=r" (x) : : : "volatile");
    x
}

/// enable device interrupts
#[inline]
pub unsafe fn intr_on() {
    w_sie(r_sie() | SIE_SEIE as usize | SIE_STIE as usize | SIE_SSIE as usize);
    w_sstatus(r_sstatus() | SSTATUS_SIE as usize);
}

/// disable device interrupts
#[inline]
pub unsafe fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as usize);
}

/// are device interrupts enabled?
#[inline]
pub unsafe fn intr_get() -> i32 {
    let mut x: usize = r_sstatus();
    (x & SSTATUS_SIE as usize != 0) as i32
}

/// read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
pub unsafe fn r_tp() -> usize {
    let mut x: usize = 0;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn r_sp() -> usize {
    let mut x: usize = 0;
    llvm_asm!("mv %0, sp" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_tp(mut x: usize) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_ra() -> usize {
    let mut x: usize = 0;
    llvm_asm!("mv %0, ra" : "=r" (x) : : : "volatile");
    x
}

/// flush the TLB.
#[inline]
pub unsafe fn sfence_vma() {
    // the zero, zero means flush all TLB entries.
    llvm_asm!("sfence.vma zero, zero" : : : : "volatile");
}

/// bytes per page
pub const PGSIZE: i32 = 4096;

/// bits of offset within a page
pub const PGSHIFT: i32 = 12;

#[inline]
pub const fn pgroundup(sz: usize) -> usize {
    sz.wrapping_add(PGSIZE as usize).wrapping_sub(1) & (!(PGSIZE - 1) as usize)
}

#[inline]
pub const fn pgrounddown(a: usize) -> usize {
    a & !(PGSIZE - 1) as usize
}

/// valid
pub const PTE_V: i64 = (1) << 0;

pub const PTE_R: i64 = (1) << 1;
pub const PTE_W: i64 = (1) << 2;
pub const PTE_X: i64 = (1) << 3;

/// 1 -> user can access
pub const PTE_U: i64 = (1) << 4;

/// shift a physical address to the right place for a PTE.
#[inline]
pub const fn pa2pte(pa: usize) -> usize {
    (pa >> 12) << 10
}

#[inline]
pub const fn pte2pa(pte: pte_t) -> usize {
    (pte >> 10) << 12
}

#[inline]
pub const fn pte_flags(pte: pte_t) -> usize {
    pte & 0x3FFusize
}

/// extract the three 9-bit page table indices from a virtual address.

/// 9 bits
pub const PXMASK: i32 = 0x1ff;

#[inline]
fn pxshift(level: i32) -> i32 {
    PGSHIFT + 9 * level
}

#[inline]
pub fn px(level: i32, va: usize) -> usize {
    (va >> pxshift(level) as usize) & PXMASK as usize
}

/// one beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub const MAXVA: i64 = (1) << (9 + 9 + 9 + 12 - 1);

pub type pte_t = usize;
pub type pde_t = usize;

/// 512 PTEs
pub type pagetable_t = *mut usize;
