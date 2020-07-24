use crate::{ libc, proc::{ cpu, mycpu } };
use core::ptr;
extern "C" {
    // pub type inode;
    // pub type file;
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
// Mutual exclusion lock.
#[derive(Copy, Clone)] 
pub struct Spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// // Per-CPU state.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct cpu {
//     pub proc_0: *mut proc_0,
//     pub scheduler: context,
//     pub noff: libc::c_int,
//     pub intena: libc::c_int,
// }
// // Saved registers for kernel context switches.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct context {
//     pub ra: uint64,
//     pub sp: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
// }
// // Per-process state
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct proc_0 {
//     pub lock: spinlock,
//     pub state: procstate,
//     pub parent: *mut proc_0,
//     pub chan: *mut libc::c_void,
//     pub killed: libc::c_int,
//     pub xstate: libc::c_int,
//     pub pid: libc::c_int,
//     pub kstack: uint64,
//     pub sz: uint64,
//     pub pagetable: pagetable_t,
//     pub tf: *mut trapframe,
//     pub context: context,
//     pub ofile: [*mut file; 16],
//     pub cwd: *mut inode,
//     pub name: [libc::c_char; 16],
// }
// // per-process data for the trap handling code in trampoline.S.
// // sits in a page by itself just under the trampoline page in the
// // user page table. not specially mapped in the kernel page table.
// // the sscratch register points here.
// // uservec in trampoline.S saves user registers in the trapframe,
// // then initializes registers from the trapframe's
// // kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// // usertrapret() and userret in trampoline.S set up
// // the trapframe's kernel_*, restore user registers from the
// // trapframe, switch to the user page table, and enter user space.
// // the trapframe includes callee-saved user registers like s0-s11 because the
// // return-to-user path via usertrapret() doesn't return through
// // the entire kernel call stack.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct trapframe {
//     pub kernel_satp: uint64,
//     pub kernel_sp: uint64,
//     pub kernel_trap: uint64,
//     pub epc: uint64,
//     pub kernel_hartid: uint64,
//     pub ra: uint64,
//     pub sp: uint64,
//     pub gp: uint64,
//     pub tp: uint64,
//     pub t0: uint64,
//     pub t1: uint64,
//     pub t2: uint64,
//     pub s0: uint64,
//     pub s1: uint64,
//     pub a0: uint64,
//     pub a1: uint64,
//     pub a2: uint64,
//     pub a3: uint64,
//     pub a4: uint64,
//     pub a5: uint64,
//     pub a6: uint64,
//     pub a7: uint64,
//     pub s2: uint64,
//     pub s3: uint64,
//     pub s4: uint64,
//     pub s5: uint64,
//     pub s6: uint64,
//     pub s7: uint64,
//     pub s8: uint64,
//     pub s9: uint64,
//     pub s10: uint64,
//     pub s11: uint64,
//     pub t3: uint64,
//     pub t4: uint64,
//     pub t5: uint64,
//     pub t6: uint64,
// }
pub type pagetable_t = *mut uint64;
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
// Supervisor Status Register, sstatus
// Previous mode, 1=Supervisor, 0=User
// Supervisor Previous Interrupt Enable
// User Previous Interrupt Enable
pub const SSTATUS_SIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
// Supervisor Interrupt Enable
// User Interrupt Enable
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
// software
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
// enable device interrupts
#[inline]
unsafe fn intr_on() {
    w_sie(
        r_sie() | SIE_SEIE as libc::c_ulong | SIE_STIE as libc::c_ulong | SIE_SSIE as libc::c_ulong,
    );
    w_sstatus(r_sstatus() | SSTATUS_SIE as libc::c_ulong);
}
// disable device interrupts
#[inline]
unsafe fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as libc::c_ulong);
}
// are device interrupts enabled?
#[inline]
unsafe fn intr_get() -> libc::c_int {
    let mut x: uint64 = r_sstatus();
    (x & SSTATUS_SIE as libc::c_ulong != 0 as libc::c_int as libc::c_ulong) as libc::c_int
}
// Mutual exclusion spin locks.
#[no_mangle]
pub unsafe fn initlock(mut lk: *mut Spinlock, mut name: *mut libc::c_char) {
    (*lk).name = name;
    (*lk).locked = 0 as libc::c_int as uint;
    (*lk).cpu = ptr::null_mut();
}
// Spinlock.c
// Acquire the lock.
// Loops (spins) until the lock is acquired.
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
// Release the lock.
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
// Check whether this cpu is holding the lock.
#[no_mangle]
pub unsafe fn holding(mut lk: *mut Spinlock) -> libc::c_int {
    let mut r: libc::c_int = 0;
    push_off();
    r = ((*lk).locked != 0 && (*lk).cpu == mycpu()) as libc::c_int;
    pop_off();
    r
}
// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
// are initially off, then push_off, pop_off leaves them off.
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