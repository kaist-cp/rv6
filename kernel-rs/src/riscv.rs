use crate::libc;

/// which hart (core) is this?
#[inline]
pub unsafe extern "C" fn r_mhartid() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
    x
}

// Machine Status Register, mstatus

// previous mode.
pub const MSTATUS_MPP_MASK: libc::c_long = (3 as libc::c_long) << 11 as libc::c_int;
// TODO: unused - #define MSTATUS_MPP_M (3L << 11)
pub const MSTATUS_MPP_S: libc::c_long = (1 as libc::c_long) << 11 as libc::c_int;
// TODO: unused - #define MSTATUS_MPP_U (0L << 11)
// machine-mode interrupt enable.
pub const MSTATUS_MIE: libc::c_long = (1 as libc::c_long) << 3 as libc::c_int;
/// machine-mode interrupt enable.
#[inline]
pub unsafe extern "C" fn r_mstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe extern "C" fn w_mstatus(mut x: u64) {
    llvm_asm!("csrw mstatus, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe extern "C" fn w_mepc(mut x: u64) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
}

// Supervisor Status Register, sstatus
// Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPP: libc::c_long = (1 as libc::c_long) << 8 as libc::c_int;
// Supervisor Previous Interrupt Enable
pub const SSTATUS_SPIE: libc::c_long = (1 as libc::c_long) << 5 as libc::c_int;
// TODO: unused - #define SSTATUS_UPIE (1L << 4) // User Previous Interrupt Enable
// Supervisor Interrupt Enable
pub const SSTATUS_SIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
// TODO: unused - #define SSTATUS_UIE (1L << 0)  // User Interrupt Enable
#[inline]
pub unsafe extern "C" fn r_sstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe extern "C" fn w_sstatus(mut x: u64) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Interrupt Pending
#[inline]
pub unsafe extern "C" fn r_sip() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe extern "C" fn w_sip(mut x: u64) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
}

// Supervisor Interrupt Enable
// external
pub const SIE_SEIE: libc::c_long = (1 as libc::c_long) << 9 as libc::c_int;
// timer
pub const SIE_STIE: libc::c_long = (1 as libc::c_long) << 5 as libc::c_int;
// software
pub const SIE_SSIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
#[inline]
pub unsafe extern "C" fn r_sie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe extern "C" fn w_sie(mut x: u64) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}

// Machine-mode Interrupt Enable
// TODO: unused - #define MIE_MEIE (1L << 11) // external
// timer
pub const MIE_MTIE: libc::c_long = (1 as libc::c_long) << 7 as libc::c_int;
// TODO: unused - #define MIE_MSIE (1L << 3)  // software
#[inline]
pub unsafe extern "C" fn r_mie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe extern "C" fn w_mie(mut x: u64) {
    llvm_asm!("csrw mie, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe extern "C" fn w_sepc(mut x: u64) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe extern "C" fn r_sepc() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}
/* TODO: unused
  // Machine Exception Delegation
  static inline u64
  r_medeleg()
  {
    u64 x;
    asm volatile("csrr %0, medeleg" : "=r" (x) );
    return x;
  }
*/
#[inline]
pub unsafe extern "C" fn w_medeleg(mut x: u64) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}
/* TODO: unused
// Machine Interrupt Delegation
static inline u64
r_mideleg()
{
  u64 x;
  asm volatile("csrr %0, mideleg" : "=r" (x) );
  return x;
}
*/
#[inline]
pub unsafe extern "C" fn w_mideleg(mut x: u64) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe extern "C" fn w_stvec(mut x: u64) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}
/* TODO: unused
static inline u64
r_stvec()
{
  u64 x;
  asm volatile("csrr %0, stvec" : "=r" (x) );
  return x;
}
*/
/// Machine-mode interrupt vector
#[inline]
pub unsafe extern "C" fn w_mtvec(mut x: u64) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
}

// use riscv's sv39 page table scheme.
pub const SATP_SV39: libc::c_long = (8 as libc::c_long) << 60 as libc::c_int;
// TODO: use in other file directly - e.g., kvminithart() in vm.rs
// #define MAKE_SATP(pagetable) (SATP_SV39 | (((u64)pagetable) >> 12))

/// supervisor address translation and protection;
/// holds the address of the page table.
#[inline]
pub unsafe extern "C" fn w_satp(mut x: u64) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}
#[inline]
pub unsafe extern "C" fn r_satp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}

/*

// Supervisor Scratch register, for early trap handler in trampoline.S.
static inline void
w_sscratch(u64 x)
{
  asm volatile("csrw sscratch, %0" : : "r" (x));
}
*/
#[inline]
pub unsafe extern "C" fn w_mscratch(mut x: u64) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Trap Cause
#[inline]
pub unsafe extern "C" fn r_scause() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Trap Value
#[inline]
pub unsafe extern "C" fn r_stval() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, stval" : "=r" (x) : : : "volatile");
    x
}
/*
TODO: unused
// Machine-mode Counter-Enable
static inline void
w_mcounteren(u64 x)
{
  asm volatile("csrw mcounteren, %0" : : "r" (x));
}

TODO: unused
static inline u64
r_mcounteren()
{
  u64 x;
  asm volatile("csrr %0, mcounteren" : "=r" (x) );
  return x;
}

TODO: unused
// machine-mode cycle counter
static inline u64
r_time()
{
  u64 x;
  asm volatile("csrr %0, time" : "=r" (x) );
  return x;
}
*/
/// enable device interrupts
#[inline]
pub unsafe extern "C" fn intr_on() {
    w_sie(
        r_sie() | SIE_SEIE as libc::c_ulong | SIE_STIE as libc::c_ulong | SIE_SSIE as libc::c_ulong,
    );
    w_sstatus(r_sstatus() | SSTATUS_SIE as libc::c_ulong);
}
/// disable device interrupts
#[inline]
pub unsafe extern "C" fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as libc::c_ulong);
}
/// are device interrupts enabled?
#[inline]
pub unsafe extern "C" fn intr_get() -> libc::c_int {
    let mut x: u64 = r_sstatus();
    (x & SSTATUS_SIE as libc::c_ulong != 0 as libc::c_int as libc::c_ulong) as libc::c_int
}

/// read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
pub unsafe extern "C" fn r_tp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}

/*
TODO: will be used in usetests.rs
static inline u64
r_sp()
{
  u64 x;
  asm volatile("mv %0, sp" : "=r" (x) );
  return x;
}
*/

#[inline]
pub unsafe extern "C" fn w_tp(mut x: u64) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}

/*
TODO: unused
static inline u64
r_ra()
{
  u64 x;
  asm volatile("mv %0, ra" : "=r" (x) );
  return x;
}
*/

/// flush the TLB.
#[inline]
pub unsafe extern "C" fn sfence_vma() {
    // the zero, zero means flush all TLB entries.
    llvm_asm!("sfence.vma zero, zero" : : : : "volatile");
}

// bytes per page
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// bits of offset within a page
pub const PGSHIFT: libc::c_int = 12 as libc::c_int;

/*
TODO: used directly in oter function e.g., uvmalloc in vm.rs
#define PGROUNDUP(sz)  (((sz)+PGSIZE-1) & ~(PGSIZE-1))
#define PGROUNDDOWN(a) (((a)) & ~(PGSIZE-1))
*/
// valid
pub const PTE_V: libc::c_long = (1 as libc::c_long) << 0 as libc::c_int;
pub const PTE_R: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
pub const PTE_W: libc::c_long = (1 as libc::c_long) << 2 as libc::c_int;
pub const PTE_X: libc::c_long = (1 as libc::c_long) << 3 as libc::c_int;
// 1 -> user can access
pub const PTE_U: libc::c_long = (1 as libc::c_long) << 4 as libc::c_int;

/*
TODO: used directly in other file e.g., vm.rs
// shift a physical address to the right place for a PTE.
#define PA2PTE(pa) ((((u64)pa) >> 12) << 10)

#define PTE2PA(pte) (((pte) >> 10) << 12)

#define PTE_FLAGS(pte) ((pte) & 0x3FF)
*/
// extract the three 9-bit page table indices from a virtual address.
// 9 bits
pub const PXMASK: libc::c_int = 0x1ff as libc::c_int;

/*
TODO: unused
#define PXSHIFT(level)  (PGSHIFT+(9*(level)))
TODO: used directly in vm.rs
#define PX(level, va) ((((u64) (va)) >> PXSHIFT(level)) & PXMASK)
*/

// one beyond the highest possible virtual address.
// MAXVA is actually one bit less than the max allowed by
// Sv39, to avoid having to sign-extend virtual addresses
// that have the high bit set.
pub const MAXVA: libc::c_long = (1 as libc::c_long)
    << (9 as libc::c_int + 9 as libc::c_int + 9 as libc::c_int + 12 as libc::c_int
        - 1 as libc::c_int);

pub type pte_t = u64;
pub type pagetable_t = *mut u64;
