//! Spin locks
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::{Guard, Lock, RawLock};
use crate::{cpu::Cpu, hal::hal};

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

    /// Check whether this cpu is holding the lock.
    /// Interrupts must be off.
    fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) == hal().cpus.current()
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
            hal().cpus.push_off();
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
                hal().cpus.current(),
                Ordering::Acquire,
                // Okay to use `Relaxed` ordering since we don't enter the critical section anyway
                // if the exchange fails.
                Ordering::Relaxed,
            )
            .is_err()
        {
            ::core::hint::spin_loop();
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
            hal().cpus.pop_off();
        }
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
