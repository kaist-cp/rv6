use crate::{
    proc::{mycpu, Cpu},
    riscv::{intr_get, intr_off, intr_on},
};
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
/// Mutual exclusion lock.
pub struct RawSpinlock {
    /// Is the lock held?
    locked: AtomicBool,

    /// For debugging:

    /// Name of lock.
    name: &'static str,

    /// The cpu holding the lock.
    cpu: *mut Cpu,
}

impl RawSpinlock {
    // TODO: transient measure
    pub const fn init(name: &'static str) -> Self {
        Self {
            locked: AtomicBool::new(false),
            name,
            cpu: ptr::null_mut(),
        }
    }

    // will remove after refactor
    pub const fn zeroed() -> Self {
        Self {
            locked: AtomicBool::new(false),
            name: "",
            cpu: ptr::null_mut(),
        }
    }

    /// Mutual exclusion spin locks.
    pub fn initlock(&mut self, name: &'static str) {
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
            panic!("acquire");
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
            panic!("release");
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
    lock: &'s mut Spinlock<T>,
    _marker: PhantomData<*const ()>,
}

pub struct Spinlock<T> {
    lock: RawSpinlock,
    data: UnsafeCell<T>,
}

impl<T> Spinlock<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinlock::init(name),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub fn lock(&mut self) -> SpinLockGuard<'_, T> {
        unsafe {
            self.lock.acquire();
        }
        SpinLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }
}

impl<T> SpinLockGuard<'_, T> {
    pub fn raw(&mut self) -> usize {
        self.lock as *const _ as usize
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.lock.lock.release() };
    }
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let old = intr_get();
    intr_off();
    if (*(mycpu())).noff == 0 {
        (*(mycpu())).interrupt_enabled = old
    }
    (*(mycpu())).noff += 1;
}
pub unsafe fn pop_off() {
    let mut c: *mut Cpu = mycpu();
    if intr_get() {
        panic!("pop_off - interruptible");
    }
    (*c).noff -= 1;
    if (*c).noff < 0 {
        panic!("pop_off");
    }
    if (*c).noff == 0 && (*c).interrupt_enabled == true {
        intr_on();
    };
}
