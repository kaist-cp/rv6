use crate::libc;
use crate::{
    printf::panic,
    proc::{cpu, mycpu},
    riscv::{intr_get, intr_off, intr_on},
};
use core::ptr;

/// Mutual exclusion lock.
#[derive(Copy, Clone)]
pub struct Spinlock {
    pub locked: u32,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}

impl Spinlock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: 0,
            name: 0 as *const libc::c_char as *mut libc::c_char,
            cpu: 0 as *const cpu as *mut cpu,
        }
    }

    /// Mutual exclusion spin locks.
    pub fn initlock(&mut self, mut name: *mut libc::c_char) {
        (*self).name = name;
        (*self).locked = 0 as i32 as u32;
        (*self).cpu = ptr::null_mut();
    }

    /// Acquire the lock.
    /// Loops (spins) until the lock is acquired.
    pub unsafe fn acquire(&mut self) {
        // disable interrupts to avoid deadlock.
        push_off();
        if holding(self) != 0 {
            panic(b"acquire\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }

        // On RISC-V, sync_lock_test_and_set turns into an atomic swap:
        //   a5 = 1
        //   s1 = &self->locked
        //   amoswap.w.aq a5, a5, (s1)
        while ::core::intrinsics::atomic_xchg_acq(&mut (*self).locked as *mut u32, 1 as i32 as u32)
            != 0 as i32 as u32
        {}

        // Tell the C compiler and the processor to not move loads or stores
        // past this point, to ensure that the critical section's memory
        // references happen after the lock is acquired.
        ::core::intrinsics::atomic_fence();

        // Record info about lock acquisition for holding() and debugging.
        (*self).cpu = mycpu();
    }
}

/// Release the lock.
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
pub unsafe fn holding(mut lk: *mut Spinlock) -> i32 {
    let mut r: i32 = 0;
    push_off();
    r = ((*lk).locked != 0 && (*lk).cpu == mycpu()) as i32;
    pop_off();
    r
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let mut old: i32 = intr_get();
    intr_off();
    if (*(mycpu())).noff == 0 as i32 {
        (*(mycpu())).intena = old
    }
    (*(mycpu())).noff += 1 as i32;
}
pub unsafe fn pop_off() {
    let mut c: *mut cpu = mycpu();
    if intr_get() != 0 {
        panic(
            b"pop_off - interruptible\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
    }
    (*c).noff -= 1 as i32;
    if (*c).noff < 0 as i32 {
        panic(b"pop_off\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*c).noff == 0 as i32 && (*c).intena != 0 {
        intr_on();
    };
}
