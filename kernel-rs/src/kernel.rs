use core::cell::UnsafeCell;
use core::fmt::{self, Write};
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};

use array_macro::array;
use pin_project::pin_project;

use crate::{
    bio::Bcache,
    console::{consoleinit, Console, Printer},
    file::{Devsw, FileTable},
    fs::{FileSystem, Itable},
    intr::HeldInterrupts,
    kalloc::Kmem,
    lock::{Sleepablelock, Spinlock},
    param::{NCPU, NDEV},
    plic::{plicinit, plicinithart},
    println,
    proc::{cpuid, Cpu, Procs, ProcsBuilder},
    trap::{trapinit, trapinithart},
    uart::Uart,
    vm::KernelMemory,
};

/// The kernel.
static mut KERNEL: KernelBuilder = KernelBuilder::zero();

/// After intialized, the kernel is safe to immutably access.
// TODO: make it unsafe
#[inline]
pub fn kernel_builder() -> &'static KernelBuilder {
    unsafe { &KERNEL }
}

#[inline]
pub unsafe fn kernel() -> &'static Kernel {
    // SAFETY: Safe to cast &KernelBuilder into &Kernel
    // since Kernel has a transparent memory layout.
    unsafe { &*(kernel_builder() as *const _ as *const _) }
}

/// Returns a pinned mutable reference to the `KERNEL`.
///
/// # Safety
///
/// The caller should make sure not to call this function multiple times.
/// All mutable accesses to the `KERNEL` must be done through this.
#[inline]
unsafe fn kernel_builder_unchecked_pin() -> Pin<&'static mut KernelBuilder> {
    // SAFETY: safe if all mutable accesses to the `KERNEL` are done through this.
    unsafe { Pin::new_unchecked(&mut KERNEL) }
}

/// # Safety
///
/// The `Kernel` is `!Unpin`, since it owns data that are `!Unpin`, such as the `bcache`.
/// Hence, all mutable accesses to the `Kernel` or its inner data that are `!Unpin` must be done using a pin.
///
/// If the `Cpu` executing the code has a non-null `Proc` pointer,
/// the `Proc` in `CurrentProc` is always valid while the `Kernel` is alive.
#[pin_project]
pub struct KernelBuilder {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Sleepablelock<Console>,

    /// TODO(https://github.com/kaist-cp/rv6/issues/298): Kernel owns uart temporarily.
    /// This might be changed after refactoring relationship between Console-Uart-Printer.
    pub uart: Uart,

    pub printer: Spinlock<Printer>,

    #[pin]
    pub kmem: Spinlock<Kmem>,

    /// The kernel's memory manager.
    memory: MaybeUninit<KernelMemory>,

    pub ticks: Sleepablelock<u32>,

    /// Current process system.
    #[pin]
    pub procs: ProcsBuilder,

    // The `Cpu` struct of the current cpu can be mutated. To do so, we need to
    // obtain mutable pointers to the elements of `cpus` from a shared reference
    // of a `Kernel`. It requires interior mutability, so we use `UnsafeCell`.
    cpus: [UnsafeCell<Cpu>; NCPU],

    #[pin]
    bcache: Bcache,

    pub devsw: [Devsw; NDEV],

    pub ftable: FileTable,

    pub itable: Itable,

    pub file_system: FileSystem,
}

#[repr(transparent)]
/// # Safety
///
/// `inner.procs` is initialized.
pub struct Kernel {
    inner: KernelBuilder,
}

impl Kernel {
    pub fn procs(&self) -> &Procs {
        // SAFETY: `self.inner.procs` is initialized according to the invariant.
        unsafe { self.inner.procs.as_procs_unchecked() }
    }
}

impl Deref for Kernel {
    type Target = KernelBuilder;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl KernelBuilder {
    const fn zero() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            console: Sleepablelock::new("CONS", Console::new()),
            uart: Uart::new(),
            printer: Spinlock::new("PRINTLN", Printer::new()),
            kmem: Spinlock::new("KMEM", unsafe { Kmem::new() }),
            memory: MaybeUninit::uninit(),
            ticks: Sleepablelock::new("time", 0),
            procs: ProcsBuilder::zero(),
            cpus: array![_ => UnsafeCell::new(Cpu::new()); NCPU],
            // SAFETY: the only way to access `bcache` is through `kernel()`, which is an immutable reference.
            bcache: unsafe { Bcache::zero() },
            devsw: [Devsw {
                read: None,
                write: None,
            }; NDEV],
            ftable: FileTable::zero(),
            itable: Itable::zero(),
            file_system: FileSystem::zero(),
        }
    }

    fn panic(&self) {
        self.panicked.store(true, Ordering::Release);
    }

    pub fn is_panicked(&self) -> bool {
        self.panicked.load(Ordering::Acquire)
    }

    /// Prints the given formatted string with the Printer.
    pub fn printer_write_fmt(&self, args: fmt::Arguments<'_>) -> fmt::Result {
        if self.is_panicked() {
            unsafe { (*self.printer.get_mut_raw()).write_fmt(args) }
        } else {
            let mut lock = self.printer.lock();
            lock.write_fmt(args)
        }
    }

    /// Return this CPU's cpu struct.
    ///
    /// It is safe to call this function with interrupts enabled, but returned address may not be the
    /// current CPU since the scheduler can move the process to another CPU on time interrupt.
    pub fn current_cpu_raw(&self) -> *mut Cpu {
        let id: usize = cpuid();
        self.cpus[id].get()
    }

    pub fn current_cpu<'a>(&'a self, _held: &'a mut HeldInterrupts) -> &'a mut Cpu {
        // SAFETY: cpu can be exclusively accessed because interrupts are disabled.
        unsafe { &mut *self.current_cpu_raw() }
    }

    /// Returns an immutable reference to the kernel's bcache.
    ///
    /// # Safety
    ///
    /// Access it only after initializing the kernel using `kernel_main()`.
    pub unsafe fn get_bcache(&self) -> &Bcache {
        &self.bcache
    }
}

/// print! macro prints to the console using printer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::kernel::kernel_builder().printer_write_fmt(format_args!($($arg)*)).unwrap();
    };
}

/// println! macro prints to the console using printer.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    // Freeze other CPUs.
    kernel_builder().panic();
    println!("{}", info);

    crate::utils::spin_loop()
}

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn kernel_main() -> ! {
    static STARTED: AtomicBool = AtomicBool::new(false);

    if cpuid() == 0 {
        let mut kernel = unsafe { kernel_builder_unchecked_pin().project() };

        // Initialize the kernel.

        // Console.
        Uart::init();
        unsafe { consoleinit(kernel.devsw) };

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        unsafe { kernel.kmem.as_mut().get_pin_mut().init() };

        // Create kernel memory manager.
        let memory =
            KernelMemory::new(kernel.kmem.as_ref().get_ref()).expect("PageTable::new failed");

        // Turn on paging.
        unsafe { kernel.memory.write(memory).init_hart() };

        // Process system.
        let procs = kernel.procs.init();

        // Trap vectors.
        trapinit();

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Set up interrupt controller.
        unsafe { plicinit() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };

        // Buffer cache.
        kernel.bcache.get_pin_mut().init();

        // Emulated hard disk.
        kernel.file_system.log.disk.get_mut().init();

        // First user process.
        procs.user_proc_init(kernel.kmem.as_ref().get_ref());

        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            spin_loop();
        }

        println!("hart {} starting", cpuid());

        // Turn on paging.
        unsafe { kernel().memory.assume_init_ref().init_hart() };

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };
    }

    unsafe { kernel().scheduler() }
}
