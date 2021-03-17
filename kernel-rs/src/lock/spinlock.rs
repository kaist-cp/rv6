//! Spin locks
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::{Guard, Lock, RawLock};
use crate::{
    kernel::kernel_builder,
    proc::Cpu,
    riscv::{intr_get, intr_off, intr_on},
};

/// Mutual exclusion lock that busy waits (spin).
pub struct RawSpinlock {
    /// Name of lock.
    name: &'static str,

    /// If the lock is held, contains the pointer of `Cpu`.
    /// Otherwise, contains null.
    ///
    /// Records info about lock acquisition for holding() and debugging.
    locked: AtomicPtr<Cpu>,
}

/// Locks that busy wait (spin).
pub type Spinlock<T> = Lock<RawSpinlock, T>;
/// Guards of `Spinlock<T>`.
pub type SpinlockGuard<'s, T> = Guard<'s, RawSpinlock, T>;

impl RawSpinlock {
    /// Mutual exclusion spin locks.
    pub const fn new(name: &'static str) -> Self {
        Self {
            locked: AtomicPtr::new(ptr::null_mut()),
            name,
        }
    }
}

impl RawLock for RawSpinlock {
    /// Acquires the lock.
    /// Loops (spins) until the lock is acquired.
    ///
    /// # Safety
    ///
    /// To ensure that all stores done in one critical section are visible in the next critical section's loads,
    /// we use an atomic exchange with `Acquire` ordering in `RawSpinlock::acquire()`,
    /// and pair it with an atomic store with `Release` ordering in `RawSpinlock::release()`.
    ///
    /// In this way, we tell the compiler/processor not to move loads (stores) that should
    /// originally happen after acquiring (before releasing) the lock to actually happen
    /// before acquiring (after releasing) the lock. Otherwise, loads could read stale values.
    ///
    /// Additionally, note that an additional fence is unneccessary due to the pair of `Acquire`/`Release` orderings.
    fn acquire(&self) {
        // Disable interrupts to avoid deadlock.
        unsafe {
            push_off();
        }
        assert!(!self.holding(), "acquire {}", self.name);

        // RISC-V supports two forms of atomic instructions, 1) load-reserved/store-conditional and 2) atomic fetch-and-op,
        // and we use the former here.
        //
        // 0x80000fdc | lr.d.aq a2,(a0)         (load-reserved, dword, acquire-ordering)
        // 0x80000fe0 | bnez    a2,0x80000fe8   (goto snez)
        // 0x80000fe2 | sc.d    a3,a1,(a0)      (store-conditional, dword)
        // 0x80000fe6 | bnez    a3,0x80000fdc   (go back to start of loop)
        // 0x80000fe8 | snez    a0,a2           (set if not zero)
        while self
            .locked
            .compare_exchange(
                ptr::null_mut(),
                // TODO: remove kernel_builder()
                kernel_builder().current_cpu(),
                Ordering::Acquire,
                // Okay to use `Relaxed` ordering since we don't enter the critical section anyway
                // if the exchange fails.
                Ordering::Relaxed,
            )
            .is_err()
        {
            spin_loop();
        }
    }

    /// Releases the lock.
    ///
    /// # Safety
    /// We use an atomic store with `Release` ordering here. See `RawSpinlock::acquire()` for more details.
    fn release(&self) {
        assert!(self.holding(), "release {}", self.name);

        // Release the lock by storing ptr::null_mut() in `self.locked`
        // using an atomic store. This is actually done using a fence in RISC-V.
        //
        // 0x80000f5c | fence   rw,w            (Enforces `Release` memory ordering)
        self.locked.store(ptr::null_mut(), Ordering::Release);
        unsafe {
            pop_off();
        }
    }

    /// Check whether this cpu is holding the lock.
    /// Interrupts must be off.
    fn holding(&self) -> bool {
        // TODO: remove kernel_builder()
        self.locked.load(Ordering::Relaxed) == kernel_builder().current_cpu()
    }
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let old = intr_get();
    unsafe { intr_off() };

    // TODO: remove kernel_builder()
    let mut cpu = kernel_builder().current_cpu();
    if unsafe { (*cpu).noff } == 0 {
        unsafe { (*cpu).interrupt_enabled = old };
    }
    unsafe { (*cpu).noff += 1 };
}

/// pop_off() should be paired with push_off().
/// See push_off() for more details.
pub unsafe fn pop_off() {
    // TODO: remove kernel_builder()
    let mut cpu: *mut Cpu = kernel_builder().current_cpu();
    assert!(!intr_get(), "pop_off - interruptible");
    assert!(unsafe { (*cpu).noff } >= 1, "pop_off");

    unsafe { (*cpu).noff -= 1 };

    if unsafe { (*cpu).noff == 0 } && unsafe { (*cpu).interrupt_enabled } {
        unsafe { intr_on() };
    }
}

impl<T> Spinlock<T> {
    /// Returns a new `Spinlock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}
