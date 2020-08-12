use crate::{
    printf::panic,
    proc::{mycpu, Cpu},
    riscv::{intr_get, intr_off, intr_on},
};
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
/// Mutual exclusion lock.
pub struct RawSpinlock {
    /// Is the lock held?
    locked: AtomicBool,

    /// For debugging:

    /// Name of lock.
    name: *mut u8,

    /// The cpu holding the lock.
    cpu: *mut Cpu,
}

impl RawSpinlock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: AtomicBool::new(false),
            name: ptr::null_mut(),
            cpu: ptr::null_mut(),
        }
    }

    /// Mutual exclusion spin locks.
    pub fn initlock(&mut self, name: *mut u8) {
        (*self).name = name;
        (*self).locked = AtomicBool::new(false);
        (*self).cpu = ptr::null_mut();
    }

    /// Acquire the lock.
    /// Loops (spins) until the lock is acquired.
    pub unsafe fn acquire(&mut self) {
        // disable interrupts to avoid deadlock.
        push_off();
        if self.holding() != 0 {
            panic(b"acquire\x00" as *const u8 as *mut u8);
        }

        // On RISC-V, sync_lock_test_and_set turns into an atomic swap:
        //   a5 = 1
        //   s1 = &self->locked
        //   amoswap.w.aq a5, a5, (s1)
        while (*self)
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {}

        // Tell the C compiler and the processor to not move loads or stores
        // past this point, to ensure that the critical section's memory
        // references happen after the lock is acquired.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();

        // Record info about lock acquisition for holding() and debugging.
        (*self).cpu = mycpu();
    }

    /// Release the lock.
    pub unsafe fn release(&mut self) {
        if self.holding() == 0 {
            panic(b"release\x00" as *const u8 as *mut u8);
        }
        (*self).cpu = ptr::null_mut();

        // Tell the C compiler and the CPU to not move loads or stores
        // past this point, to ensure that all the stores in the critical
        // section are visible to other CPUs before the lock is released.
        // On RISC-V, this turns into a fence instruction.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();

        // Release the lock, equivalent to lk->locked = 0.
        // This code doesn't use a C assignment, since the C standard
        // implies that an assignment might be implemented with
        // multiple store instructions.
        // On RISC-V, sync_lock_release turns into an atomic swap:
        //   s1 = &lk->locked
        //   amoswap.w zero, zero, (s1)
        (*self).locked.store(false, Ordering::Release);
        pop_off();
    }

    /// Check whether this cpu is holding the lock.
    pub unsafe fn holding(&mut self) -> i32 {
        push_off();
        let r: i32 = ((*self).locked.load(Ordering::Acquire) && (*self).cpu == mycpu()) as i32;
        pop_off();
        r
    }
}

pub struct SpinLockGuard<'s, T> {
    lock: &'s mut NewSpinlock<T>,
    // token: &'s RawLock<T>,
    _marker: PhantomData<*const ()>,
}

pub struct NewSpinlock<T> {
    lock: RawSpinlock,
    data: UnsafeCell<T>,
}

impl<T> NewSpinlock<T> {
    pub fn new(data: T) -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub unsafe fn lock(&mut self) -> SpinLockGuard<'_, T> {
        self.lock.acquire();
        SpinLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    pub unsafe fn unlock(&mut self) {
        self.lock.release();
    }
}

impl<T> SpinLockGuard<'_, T> {
    pub fn raw(&mut self) -> usize {
        self.lock as *const _ as usize
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.lock.unlock() };
    }
}

// 여기부터는 원래 Spinlock

/// Mutual exclusion lock.
pub struct Spinlock {
    /// Is the lock held?
    locked: AtomicBool,

    /// For debugging:

    /// Name of lock.
    name: *mut u8,

    /// The cpu holding the lock.
    cpu: *mut Cpu,
}

impl Spinlock {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            locked: AtomicBool::new(false),
            name: ptr::null_mut(),
            cpu: ptr::null_mut(),
        }
    }

    /// Mutual exclusion spin locks.
    pub fn initlock(&mut self, name: *mut u8) {
        (*self).name = name;
        (*self).locked = AtomicBool::new(false);
        (*self).cpu = ptr::null_mut();
    }

    /// Acquire the lock.
    /// Loops (spins) until the lock is acquired.
    pub unsafe fn acquire(&mut self) {
        // disable interrupts to avoid deadlock.
        push_off();
        if self.holding() != 0 {
            panic(b"acquire\x00" as *const u8 as *mut u8);
        }

        // On RISC-V, sync_lock_test_and_set turns into an atomic swap:
        //   a5 = 1
        //   s1 = &self->locked
        //   amoswap.w.aq a5, a5, (s1)
        while (*self)
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {}

        // Tell the C compiler and the processor to not move loads or stores
        // past this point, to ensure that the critical section's memory
        // references happen after the lock is acquired.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();

        // Record info about lock acquisition for holding() and debugging.
        (*self).cpu = mycpu();
    }

    /// Release the lock.
    pub unsafe fn release(&mut self) {
        if self.holding() == 0 {
            panic(b"release\x00" as *const u8 as *mut u8);
        }
        (*self).cpu = ptr::null_mut();

        // Tell the C compiler and the CPU to not move loads or stores
        // past this point, to ensure that all the stores in the critical
        // section are visible to other CPUs before the lock is released.
        // On RISC-V, this turns into a fence instruction.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();

        // Release the lock, equivalent to lk->locked = 0.
        // This code doesn't use a C assignment, since the C standard
        // implies that an assignment might be implemented with
        // multiple store instructions.
        // On RISC-V, sync_lock_release turns into an atomic swap:
        //   s1 = &lk->locked
        //   amoswap.w zero, zero, (s1)
        (*self).locked.store(false, Ordering::Release);
        pop_off();
    }

    /// Check whether this cpu is holding the lock.
    pub unsafe fn holding(&mut self) -> i32 {
        push_off();
        let r: i32 = ((*self).locked.load(Ordering::Acquire) && (*self).cpu == mycpu()) as i32;
        pop_off();
        r
    }
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let old: i32 = intr_get();
    intr_off();
    if (*(mycpu())).noff == 0 {
        (*(mycpu())).intena = old
    }
    (*(mycpu())).noff += 1;
}
pub unsafe fn pop_off() {
    let mut c: *mut Cpu = mycpu();
    if intr_get() != 0 {
        panic(b"pop_off - interruptible\x00" as *const u8 as *mut u8);
    }
    (*c).noff -= 1;
    if (*c).noff < 0 {
        panic(b"pop_off\x00" as *const u8 as *mut u8);
    }
    if (*c).noff == 0 && (*c).intena != 0 {
        intr_on();
    };
}
