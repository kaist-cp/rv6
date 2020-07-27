use crate::printf::panic;
use crate::{
    libc,
    proc::{cpu, mycpu},
};
use core::ptr;
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
/// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
pub const SSTATUS_SIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
/// Supervisor Interrupt Enable
/// User Interrupt Enable
#[inline]
unsafe fn r_sstatus() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe fn w_sstatus(mut x: uint64) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}
// Supervisor Interrupt Enable
pub const SIE_SEIE: libc::c_long = (1 as libc::c_long) << 9 as libc::c_int;
// external
pub const SIE_STIE: libc::c_long = (1 as libc::c_long) << 5 as libc::c_int;
// timer
pub const SIE_SSIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
/// software
#[inline]
unsafe fn r_sie() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe fn w_sie(mut x: uint64) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}
/// enable device interrupts
#[inline]
unsafe fn intr_on() {
    w_sie(
        r_sie() | SIE_SEIE as libc::c_ulong | SIE_STIE as libc::c_ulong | SIE_SSIE as libc::c_ulong,
    );
    w_sstatus(r_sstatus() | SSTATUS_SIE as libc::c_ulong);
}
/// disable device interrupts
#[inline]
unsafe fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as libc::c_ulong);
}
/// are device interrupts enabled?
#[inline]
unsafe fn intr_get() -> libc::c_int {
    let mut x: uint64 = r_sstatus();
    (x & SSTATUS_SIE as libc::c_ulong != 0 as libc::c_int as libc::c_ulong) as libc::c_int
}
/// Mutual exclusion spin locks.
#[no_mangle]
pub unsafe fn initlock(mut lk: *mut Spinlock, mut name: *mut libc::c_char) {
    (*lk).name = name;
    (*lk).locked = 0 as libc::c_int as uint;
    (*lk).cpu = ptr::null_mut();
}
// Spinlock.c
/// Acquire the lock.
/// Loops (spins) until the lock is acquired.
#[no_mangle]
pub unsafe fn acquire(mut lk: *mut Spinlock) {
    push_off(); // disable interrupts to avoid deadlock.
    if holding(lk) != 0 {
        panic(b"acquire\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // On RISC-V, sync_lock_test_and_set turns into an atomic swap:
    //   a5 = 1
    //   s1 = &lk->locked
    //   amoswap.w.aq a5, a5, (s1)
    while ::core::intrinsics::atomic_xchg_acq(
        &mut (*lk).locked as *mut uint,
        1 as libc::c_int as uint,
    ) != 0 as libc::c_int as libc::c_uint
    {}
    // Tell the C compiler and the processor to not move loads or stores
    // past this point, to ensure that the critical section's memory
    // references happen after the lock is acquired.
    ::core::intrinsics::atomic_fence();
    // Record info about lock acquisition for holding() and debugging.
    (*lk).cpu = mycpu();
}
/// Release the lock.
#[no_mangle]
pub unsafe fn release(mut lk: *mut Spinlock) {
    if holding(lk) == 0 {
        panic(b"release\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    (*lk).cpu = ptr::null_mut();
    // Tell the C compiler and the CPU to not move loads or stores
    // past this point, to ensure that all the stores in the critical
    // section are visible to other CPUs before the lock is released.
    // On RISC-V, this turns into a fence instruction.
    ::core::intrinsics::atomic_fence();
    // Release the lock, equivalent to lk->locked = 0.
    // This code doesn't use a C assignment, since the C standard
    // implies that an assignment might be implemented with
    // multiple store instructions.
    // On RISC-V, sync_lock_release turns into an atomic swap:
    //   s1 = &lk->locked
    //   amoswap.w zero, zero, (s1)
    ::core::intrinsics::atomic_store_rel(&mut (*lk).locked, 0);
    pop_off();
}
/// Check whether this cpu is holding the lock.
#[no_mangle]
pub unsafe fn holding(mut lk: *mut Spinlock) -> libc::c_int {
    let mut r: libc::c_int = 0;
    push_off();
    r = ((*lk).locked != 0 && (*lk).cpu == mycpu()) as libc::c_int;
    pop_off();
    r
}
/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
#[no_mangle]
pub unsafe fn push_off() {
    let mut old: libc::c_int = intr_get();
    intr_off();
    if (*(mycpu())).noff == 0 as libc::c_int {
        (*(mycpu())).intena = old
    }
    (*(mycpu())).noff += 1 as libc::c_int;
}
#[no_mangle]
pub unsafe fn pop_off() {
    let mut c: *mut cpu = mycpu();
    if intr_get() != 0 {
        panic(
            b"pop_off - interruptible\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
    }
    (*c).noff -= 1 as libc::c_int;
    if (*c).noff < 0 as libc::c_int {
        panic(b"pop_off\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*c).noff == 0 as libc::c_int && (*c).intena != 0 {
        intr_on();
    };
}
