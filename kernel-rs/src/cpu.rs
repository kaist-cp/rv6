use core::{cell::UnsafeCell, ptr};

use array_macro::array;

use crate::{
    arch::riscv::r_tp,
    arch::riscv::{intr_get, intr_off, intr_on},
    param::NCPU,
    proc::{Context, Proc},
};

pub static CPUS: Cpus = Cpus::new();

// The `Cpu` struct of the current cpu can be mutated. To do so, we need to
// obtain mutable pointers to `Cpu`s from a shared reference of a `Cpus`.
// It requires interior mutability, so we use `UnsafeCell`.
pub struct Cpus([UnsafeCell<Cpu>; NCPU]);

// SAFETY: each thread access the cpu struct of the cpu on which it's running.
unsafe impl Sync for Cpus {}

impl Cpus {
    const fn new() -> Self {
        Self(array![_ => UnsafeCell::new(Cpu::new()); NCPU])
    }
}

impl Cpus {
    /// Return this CPU's cpu struct.
    ///
    /// It is safe to call this function with interrupts enabled, but returned address may not be the
    /// current CPU since the scheduler can move the process to another CPU on time interrupt.
    pub fn current(&self) -> *mut Cpu {
        let id: usize = cpuid();
        self.0[id].get()
    }

    /// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
    /// it takes two pop_off()s to undo two push_off()s. Also, if interrupts
    /// are initially off, then push_off, pop_off leaves them off.
    pub unsafe fn push_off(&self) {
        let old = intr_get();
        unsafe { intr_off() };
        unsafe { (*self.current()).push_off(old) };
    }

    /// pop_off() should be paired with push_off().
    /// See push_off() for more details.
    pub unsafe fn pop_off(&self) {
        assert!(!intr_get(), "pop_off - interruptible");
        unsafe { (*self.current()).pop_off() };
    }
}

/// Per-CPU-state.
pub struct Cpu {
    /// The process running on this cpu, or null.
    pub proc: *const Proc,

    /// swtch() here to enter scheduler().
    pub context: Context,

    /// Depth of push_off() nesting.
    noff: i32,

    /// Were interrupts enabled before push_off()?
    interrupt_enabled: bool,
}

impl Cpu {
    const fn new() -> Self {
        Self {
            proc: ptr::null_mut(),
            context: Context::new(),
            noff: 0,
            interrupt_enabled: false,
        }
    }

    unsafe fn push_off(&mut self, old: bool) {
        if self.noff == 0 {
            self.interrupt_enabled = old;
        }
        self.noff += 1;
    }

    unsafe fn pop_off(&mut self) {
        assert!(self.noff >= 1, "pop_off");
        self.noff -= 1;
        if self.noff == 0 && self.interrupt_enabled {
            unsafe { intr_on() };
        }
    }

    pub fn noff(&self) -> i32 {
        self.noff
    }

    pub fn get_interrupt(&self) -> bool {
        self.interrupt_enabled
    }

    pub fn set_interrupt(&mut self, interrupt: bool) {
        self.interrupt_enabled = interrupt;
    }
}

/// Return this CPU's ID.
///
/// It is safe to call this function with interrupts enabled, but the returned id may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn cpuid() -> usize {
    r_tp()
}
