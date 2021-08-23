use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    ptr::{self, NonNull},
};

use array_macro::array;

use crate::{
    arch::interface::{ContextManager, ProcManager, TrapManager},
    arch::TargetArch,
    param::NCPU,
    proc::Proc,
};

// The `Cpu` struct of the current cpu can be mutated. To do so, we need to
// obtain mutable pointers to `Cpu`s from a shared reference of a `Cpus`.
// It requires interior mutability, so we use `UnsafeCell`.
pub struct Cpus([UnsafeCell<Cpu>; NCPU]);

/// # Safety
///
/// Interrupts are disabled.
// One private zero-sized field prevents `HeldInterrupts` from being constructed outside this
// module.
pub struct HeldInterrupts(());

impl HeldInterrupts {
    fn new() -> Self {
        TargetArch::intr_off();
        HeldInterrupts(())
    }
}

// SAFETY: each thread access the cpu struct of the cpu on which it's running.
unsafe impl Sync for Cpus {}

impl Cpus {
    pub const fn new() -> Self {
        Self(array![_ => UnsafeCell::new(Cpu::new()); NCPU])
    }
}

impl Cpus {
    /// Return this CPU's cpu struct.
    ///
    /// It is safe to call this function with interrupts enabled, but returned address may not be the
    /// current CPU since the scheduler can move the process to another CPU on time interrupt.
    pub fn current_raw(&self) -> *mut Cpu {
        let id: usize = cpuid();
        self.0[id].get()
    }

    /// Returns a `CpuMut` to the current CPU.
    ///
    /// # Safety
    ///
    /// The returned `CpuMut` must live while interrupts are disabled.
    pub unsafe fn current_unchecked(&self) -> CpuMut<'_> {
        // SAFETY: `self.current_raw()` is always nonnull.
        let ptr = unsafe { NonNull::new_unchecked(self.current_raw()) };
        // SAFETY:
        // * safety condition of this method.
        // * `ptr` refers to the current CPU.
        unsafe { CpuMut::new_unchecked(ptr) }
    }

    /// Returns a `CpuMut` to the current CPU. Since the returned `CpuMut` cannot outlive a given
    /// `HeldInterrupts`, it is guaranteed that the `CpuMut` always refers to the current CPU.
    /// However, there can be other `CpuMut`s referring to the same CPU. Thus, this method returns
    /// a `CpuMut` instead of `&mut Cpu`.
    pub fn current<'s>(&'s self, _: &'s HeldInterrupts) -> CpuMut<'s> {
        // SAFETY: `HeldInterrupts` guarantees that interrupts are disabled.
        unsafe { self.current_unchecked() }
    }

    /// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
    /// It takes two pop_off()s to undo two push_off()s. Also, if interrupts
    /// are initially off, then push_off, pop_off leaves them off.
    pub fn push_off(&self) -> HeldInterrupts {
        let old = TargetArch::intr_get();
        let intr = HeldInterrupts::new();
        let cpu = self.current(&intr);
        cpu.push_off(old);
        intr
    }

    /// pop_off() should be paired with push_off().
    /// See push_off() for more details.
    ///
    /// # Safety
    ///
    /// It may turn on interrupt, so callers must ensure that calling this method does not incur
    /// data race.
    pub unsafe fn pop_off(&self, intr: HeldInterrupts) {
        assert!(!TargetArch::intr_get(), "pop_off: interruptible");
        let cpu = self.current(&intr);
        // SAFETY: safety condition of this method.
        unsafe {
            cpu.pop_off();
        }
    }
}

/// Per-CPU-state.
pub struct Cpu {
    /// The process running on this cpu, or null.
    proc: *const Proc,

    /// swtch() here to enter scheduler().
    context: <TargetArch as ProcManager>::Context,

    /// Depth of push_off() nesting.
    noff: u32,

    /// Were interrupts enabled before push_off()?
    interrupt_enabled: bool,
}

impl Cpu {
    const fn new() -> Self {
        Self {
            proc: ptr::null_mut(),
            context: <TargetArch as ProcManager>::Context::new(),
            noff: 0,
            interrupt_enabled: false,
        }
    }
}

/// `CpuMut` allows safe shared mutable accesses to `Cpu`. It is similar to `&Cell<Cpu>`.
///
/// # Safety
///
/// `ptr` refers to the current CPU.
pub struct CpuMut<'s> {
    ptr: NonNull<Cpu>,
    _marker: PhantomData<&'s Cell<Cpu>>,
}

impl CpuMut<'_> {
    /// # Safety
    ///
    /// * `ptr` must refer to the current CPU.
    /// * The returned `CpuMut` must live while interrupts are disabled.
    unsafe fn new_unchecked(ptr: NonNull<Cpu>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    fn ptr(&self) -> *mut Cpu {
        self.ptr.as_ptr()
    }

    pub fn context_raw_mut(&self) -> *mut <TargetArch as ProcManager>::Context {
        // SAFETY: invariant of `CpuMut`
        unsafe { &raw mut (*self.ptr()).context }
    }

    pub fn get_proc(&self) -> *const Proc {
        // SAFETY: invariant of `CpuMut`
        unsafe { (*self.ptr.as_ptr()).proc }
    }

    pub fn set_proc(&self, proc: *const Proc) {
        // SAFETY: invariant of `CpuMut`
        unsafe {
            (*self.ptr.as_ptr()).proc = proc;
        }
    }

    pub fn get_noff(&self) -> u32 {
        // SAFETY: invariant of `CpuMut`
        unsafe { (*self.ptr()).noff }
    }

    fn set_noff(&self, noff: u32) {
        // SAFETY: invariant of `CpuMut`
        unsafe {
            (*self.ptr()).noff = noff;
        }
    }

    pub fn get_interrupt(&self) -> bool {
        // SAFETY: invariant of `CpuMut`
        unsafe { (*self.ptr()).interrupt_enabled }
    }

    pub fn set_interrupt(&self, interrupt: bool) {
        // SAFETY: invariant of `CpuMut`
        unsafe {
            (*self.ptr()).interrupt_enabled = interrupt;
        }
    }

    fn push_off(&self, old: bool) {
        let noff = self.get_noff();
        if noff == 0 {
            self.set_interrupt(old);
        }
        self.set_noff(noff + 1);
    }

    /// # Safety
    ///
    /// It may turn on interrupt, so callers must ensure that calling this method does not incur
    /// data race.
    unsafe fn pop_off(&self) {
        let noff = self.get_noff();
        assert!(noff >= 1, "pop_off");
        self.set_noff(noff - 1);
        if noff == 1 && self.get_interrupt() {
            // SAFETY: safety condition of this method.
            unsafe { TargetArch::intr_on() };
        }
    }
}

/// Return this CPU's ID.
///
/// It is safe to call this function with interrupts enabled, but the returned id may not be the
/// current CPU since the scheduler can move the process to another CPU on time interrupt.
pub fn cpuid() -> usize {
    TargetArch::cpu_id()
}
