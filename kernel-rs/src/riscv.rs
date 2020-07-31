/// which hart (core) is this?
#[inline]
pub unsafe fn r_mhartid() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
    x
}

// Machine Status Register, mstatus

// previous mode.
pub const MSTATUS_MPP_MASK: i64 = (3 as i64) << 11 as i32;
pub const MSTATUS_MPP_M: i64 = (3 as i64) << 11 as i32;
pub const MSTATUS_MPP_S: i64 = (1 as i64) << 11 as i32;
pub const MSTATUS_MPP_U: i64 = (0 as i64) << 11 as i32;
// machine-mode interrupt enable.
pub const MSTATUS_MIE: i64 = (1 as i64) << 3 as i32;
/// machine-mode interrupt enable.
#[inline]
pub unsafe fn r_mstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_mstatus(mut x: u64) {
    llvm_asm!("csrw mstatus, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_mepc(mut x: u64) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Status Register, sstatus
/// Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPP: i64 = (1 as i64) << 8 as i32;
/// Supervisor Previous Interrupt Enable
pub const SSTATUS_SPIE: i64 = (1 as i64) << 5 as i32;
/// User Previous Interrupt Enable
pub const SSTATUS_UPIE: i64 = (1 as i64) << 4 as i32;
/// Supervisor Interrupt Enable
pub const SSTATUS_SIE: i64 = (1 as i64) << 1 as i32;
/// User Interrupt Enable
pub const SSTATUS_UIE: i64 = (1 as i64) << 0 as i32;
#[inline]
pub unsafe fn r_sstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sstatus(mut x: u64) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Interrupt Pending
#[inline]
pub unsafe fn r_sip() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sip(mut x: u64) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Interrupt Enable
/// external
pub const SIE_SEIE: i64 = (1 as i64) << 9 as i32;
/// timer
pub const SIE_STIE: i64 = (1 as i64) << 5 as i32;
/// software
pub const SIE_SSIE: i64 = (1 as i64) << 1 as i32;
#[inline]
pub unsafe fn r_sie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sie(mut x: u64) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}

/// Machine-mode Interrupt Enable
/// external
pub const MIE_MEIE: i64 = (1 as i64) << 11 as i32;
/// timer
pub const MIE_MTIE: i64 = (1 as i64) << 7 as i32;
/// software
pub const MIE_MSIE: i64 = (1 as i64) << 3 as i32;
#[inline]
pub unsafe fn r_mie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_mie(mut x: u64) {
    llvm_asm!("csrw mie, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_sepc(mut x: u64) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe fn r_sepc() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}
/// Machine Exception Delegation
#[inline]
pub unsafe fn r_medeleg() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr %0, medeleg" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_medeleg(mut x: u64) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}
/// Machine Interrupt Delegation
#[inline]
pub unsafe fn r_mideleg() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr %0, mideleg" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_mideleg(mut x: u64) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe fn w_stvec(mut x: u64) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe fn r_stvec() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr %0, stvec" : "=r" (x) : : : "volatile");
    x
}
/// Machine-mode interrupt vector
#[inline]
pub unsafe fn w_mtvec(mut x: u64) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
}

/// use riscv's sv39 page table scheme.
pub const SATP_SV39: i64 = (8 as i64) << 60 as i32;
pub const fn make_satp(pagetable: u64) -> u64 {
    SATP_SV39 as u64 | pagetable >> 12 as i32
}

/// supervisor address translation and protection;
/// holds the address of the page table.
#[inline]
pub unsafe fn w_satp(mut x: u64) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe fn r_satp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Scratch register, for early trap handler in trampoline.S.
#[inline]
pub unsafe fn w_sscratch(mut x: u64) {
    llvm_asm!("csrw sscratch, %0" : : "r" (x) : : : "volatile");
}
#[inline]
pub unsafe fn w_mscratch(mut x: u64) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Trap Cause
#[inline]
pub unsafe fn r_scause() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Trap Value
#[inline]
pub unsafe fn r_stval() -> u64 {
    let mut x: u64 = 0;
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
    w_sie(r_sie() | SIE_SEIE as u64 | SIE_STIE as u64 | SIE_SSIE as u64);
    w_sstatus(r_sstatus() | SSTATUS_SIE as u64);
}
/// disable device interrupts
#[inline]
pub unsafe fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as u64);
}
/// are device interrupts enabled?
#[inline]
pub unsafe fn intr_get() -> i32 {
    let mut x: u64 = r_sstatus();
    (x & SSTATUS_SIE as u64 != 0 as i32 as u64) as i32
}
/// read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
pub unsafe fn r_tp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn r_sp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("mv %0, sp" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_tp(mut x: u64) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe fn r_ra() -> u64 {
    let mut x: u64 = 0;
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
pub const PGSIZE: i32 = 4096 as i32;
/// bits of offset within a page
pub const PGSHIFT: i32 = 12 as i32;
pub const fn pgroundup(sz: u64) -> u64 {
    sz.wrapping_add(PGSIZE as u64).wrapping_sub(1 as i32 as u64) & (!(PGSIZE - 1 as i32) as u64)
}
pub const fn pgrounddown(a: u64) -> u64 {
    a & !(PGSIZE - 1 as i32) as u64
}
/*
TODO: used directly in oter function e.g., uvmalloc in vm.rs
#define PGROUNDUP(sz)  (((sz)+PGSIZE-1) & ~(PGSIZE-1))
#define PGROUNDDOWN(a) (((a)) & ~(PGSIZE-1))
*/
/// valid
pub const PTE_V: i64 = (1 as i64) << 0 as i32;
pub const PTE_R: i64 = (1 as i64) << 1 as i32;
pub const PTE_W: i64 = (1 as i64) << 2 as i32;
pub const PTE_X: i64 = (1 as i64) << 3 as i32;
/// 1 -> user can access
pub const PTE_U: i64 = (1 as i64) << 4 as i32;
/// shift a physical address to the right place for a PTE.
pub const fn pa2pte(pa: u64) -> u64 {
    (pa >> 12 as i32) << 10 as i32
}
pub const fn pte2pa(pte: pte_t) -> u64 {
    (pte >> 10 as i32) << 12 as i32
}
pub const fn pte_flags(pte: pte_t) -> u64 {
    pte & 0x3ff as i32 as u64
}
/*
TODO: used directly in other file e.g., vm.rs

#define PA2PTE(pa) ((((u64)pa) >> 12) << 10)

#define PTE2PA(pte) (((pte) >> 10) << 12)

#define PTE_FLAGS(pte) ((pte) & 0x3FF)
*/
/// extract the three 9-bit page table indices from a virtual address.
/// 9 bits
pub const PXMASK: i32 = 0x1ff as i32;

fn pxshift(level: i32) -> i32 {
    PGSHIFT + 9 * level
}

pub fn px(level: i32, va: u64) -> u64 {
    (va >> pxshift(level) as u64) & PXMASK as u64
}
/*
TODO: unused
#define PXSHIFT(level)  (PGSHIFT+(9*(level)))
TODO: used directly in vm.rs
#define PX(level, va) ((((u64) (va)) >> PXSHIFT(level)) & PXMASK)
*/

/// one beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub const MAXVA: i64 = (1 as i64) << (9 as i32 + 9 as i32 + 9 as i32 + 12 as i32 - 1 as i32);

pub type pte_t = u64;
pub type pagetable_t = *mut u64;
