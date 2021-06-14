//! Spin locks
use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::{Guard, Lock, RawLock};
use crate::{
    cpu::{Cpu, HeldInterrupts},
    hal::hal,
};

/// Mutual exclusion lock that busy waits (spin).
pub struct RawSpinLock {
    /// Name of lock.
    name: &'static str,

    /// If the lock is held, contains the pointer of `Cpu`.
    /// Otherwise, contains null.
    ///
    /// Records info about lock acquisition for holding() and debugging.
    locked: AtomicPtr<Cpu>,
    intr: Cell<MaybeUninit<HeldInterrupts>>,
}

/// Locks that busy wait (spin).
pub type SpinLock<T> = Lock<RawSpinLock, T>;
/// Guards of `SpinLock<T>`.
pub type SpinLockGuard<'s, T> = Guard<'s, RawSpinLock, T>;

impl RawSpinLock {
    /// Mutual exclusion spin locks.
    pub const fn new(name: &'static str) -> Self {
        Self {
            locked: AtomicPtr::new(ptr::null_mut()),
            name,
            intr: Cell::new(MaybeUninit::uninit()),
        }
    }

    /// Check whether this cpu is holding the lock.
    /// Interrupts must be off.
    fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) == hal().cpus().current_raw()
    }
}

impl RawLock for RawSpinLock {
    /// Acquires the lock.
    /// Loops (spins) until the lock is acquired.
    ///
    /// # Safety
    ///
    /// To ensure that all stores done in one critical section are visible in the next critical section's loads,
    /// we use an atomic exchange with `Acquire` ordering in `RawSpinLock::acquire()`,
    /// and pair it with an atomic store with `Release` ordering in `RawSpinLock::release()`.
    ///
    /// In this way, we tell the compiler/processor not to move loads (stores) that should
    /// originally happen after acquiring (before releasing) the lock to actually happen
    /// before acquiring (after releasing) the lock. Otherwise, loads could read stale values.
    ///
    /// Additionally, note that an additional fence is unneccessary due to the pair of `Acquire`/`Release` orderings.
    fn acquire(&self) {
        // Disable interrupts to avoid deadlock.
        let intr = hal().cpus().push_off();
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
                hal().cpus().current_raw(),
                Ordering::Acquire,
                // Okay to use `Relaxed` ordering since we don't enter the critical section anyway
                // if the exchange fails.
                Ordering::Relaxed,
            )
            .is_err()
        {
            ::core::hint::spin_loop();
        }

        self.intr.set(MaybeUninit::new(intr));
    }

    /// Releases the lock.
    ///
    /// # Safety
    /// We use an atomic store with `Release` ordering here. See `RawSpinLock::acquire()` for more details.
    fn release(&self) {
        assert!(self.holding(), "release {}", self.name);

        // Release the lock by storing ptr::null_mut() in `self.locked`
        // using an atomic store. This is actually done using a fence in RISC-V.
        //
        // 0x80000f5c | fence   rw,w            (Enforces `Release` memory ordering)
        self.locked.store(ptr::null_mut(), Ordering::Release);
        let intr = unsafe { self.intr.replace(MaybeUninit::uninit()).assume_init_read() };
        unsafe { hal().cpus().pop_off(intr) };
    }
}

impl<T> SpinLock<T> {
    /// Returns a new `SpinLock` with name `name` and data `data`.
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinLock::new(name),
            data: UnsafeCell::new(data),
        }
    }
}
