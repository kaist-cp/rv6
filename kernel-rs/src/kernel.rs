use core::fmt::{self, Write};
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};

use pin_project::pin_project;

use crate::{
    arch::{
        memlayout::UART0,
        plic::{plicinit, plicinithart},
    },
    bio::Bcache,
    console::{Console, Printer},
    cpu::cpuid,
    file::{Devsw, FileTable},
    fs::{FileSystem, Itable},
    kalloc::Kmem,
    lock::{Sleepablelock, Spinlock},
    param::NDEV,
    println,
    proc::{Procs, ProcsBuilder},
    trap::{trapinit, trapinithart},
    util::{branded::Branded, spin_loop},
    vm::KernelMemory,
};

/// The kernel.
static mut KERNEL: KernelBuilder = KernelBuilder::new();

/// After intialized, the kernel is safe to immutably access.
// TODO(https://github.com/kaist-cp/rv6/issues/515): make it unsafe
#[inline]
pub fn kernel_builder<'s>() -> &'s KernelBuilder {
    unsafe { &KERNEL }
}

/// Creates a `KernelRef` that has a unique, invariant `'id` and points to the `Kernel`.
/// The `KernelRef` can be used only inside the given closure.
///
/// # Safety
///
/// Use this only after the `Kernel` is initialized.
pub unsafe fn kernel_ref<'s, F: for<'new_id> FnOnce(KernelRef<'new_id, 's>) -> R, R>(f: F) -> R {
    // SAFETY: Safe to cast &KernelBuilder into &Kernel
    // since Kernel has a transparent memory layout.
    let kernel = unsafe { &*(kernel_builder() as *const _ as *const _) };

    Branded::new(kernel, |k| f(KernelRef(k)))
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
    pub console: Console,

    pub printer: Spinlock<Printer>,

    #[pin]
    pub kmem: Spinlock<Kmem>,

    /// The kernel's memory manager.
    memory: MaybeUninit<KernelMemory>,

    ticks: Sleepablelock<u32>,

    /// Current process system.
    #[pin]
    pub procs: ProcsBuilder,

    #[pin]
    bcache: Bcache,

    devsw: [Devsw; NDEV],

    pub ftable: FileTable,

    pub itable: Itable,

    // TODO(https://github.com/kaist-cp/rv6/issues/516)
    // Make this private and always use `KernelRef::fs` instead.
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

/// A branded reference to a `Kernel`.
///
/// # Safety
///
/// The `'id` is always different between different `Kernel` instances.
#[derive(Clone, Copy)]
pub struct KernelRef<'id, 's>(Branded<'id, &'s Kernel>);

impl<'id, 's> KernelRef<'id, 's> {
    /// Returns a `Branded` that wraps `data` and has the same `'id` tag with `self`.
    ///
    /// # Note
    ///
    /// This lets you add the `'id` tag to any kind of data `T`.
    /// Therefore, you should always wrap the returned `Branded` with your own wrapper (e.g. `ProcsRef`),
    /// and make sure that wrapper can be obtained only through a controlled way (e.g. Using `KernelRef::procs` method,
    /// you can get a `ProcsRef<'id, 's>` only from a `KernelRef<'id, 's>` that has the same `'id` tag).
    /// That is,
    /// * One can obtain a `Branded<'id, T>` for any data, but
    /// * One can obtain the wrapper type only through a restricted way.
    pub fn brand<T>(&self, data: T) -> Branded<'id, T> {
        self.0.brand(data)
    }

    /// Returns a reference to the kernel's ticks.
    pub fn ticks(&self) -> &'s Sleepablelock<u32> {
        &self.0.ticks
    }

    /// Returns a reference to the kernel's `Devsw` array.
    pub fn devsw(&self) -> &'s [Devsw; NDEV] {
        &self.0.devsw
    }

    /// Returns a reference to the kernel's `FileSystem`.
    // Need this to prevent lifetime confusions.
    pub fn fs(&self) -> &'s FileSystem {
        &self.0.file_system
    }
}

impl<'id, 's> Deref for KernelRef<'id, 's> {
    type Target = Kernel;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl KernelBuilder {
    const fn new() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            console: unsafe { Console::new(UART0) },
            printer: Spinlock::new("PRINTLN", Printer::new()),
            kmem: Spinlock::new("KMEM", unsafe { Kmem::new() }),
            memory: MaybeUninit::uninit(),
            ticks: Sleepablelock::new("time", 0),
            procs: ProcsBuilder::zero(),
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

    /// Initializes the kernel.
    ///
    /// # Safety
    ///
    /// This method should be called only once by the hart 0.
    unsafe fn init(self: Pin<&mut Self>) {
        let mut this = self.project();

        // Console.
        this.console.init(&mut this.devsw);

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        unsafe { this.kmem.as_mut().get_pin_mut().init() };

        // Create kernel memory manager.
        let memory =
            KernelMemory::new(this.kmem.as_ref().get_ref()).expect("PageTable::new failed");

        // Turn on paging.
        unsafe { this.memory.write(memory).init_hart() };

        // Process system.
        let procs = this.procs.init();

        // Trap vectors.
        trapinit();

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Set up interrupt controller.
        unsafe { plicinit() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };

        // Buffer cache.
        this.bcache.get_pin_mut().init();

        // Emulated hard disk.
        this.file_system.log.disk.get_mut().init();

        // First user process.
        procs.user_proc_init(this.kmem.as_ref().get_ref());
    }

    /// Initializes the kernel for a hart.
    ///
    /// # Safety
    ///
    /// This method should be called only once by each hart.
    unsafe fn inithart(&self) {
        println!("hart {} starting", cpuid());

        // Turn on paging.
        unsafe { self.memory.assume_init_ref().init_hart() };

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };
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

    spin_loop()
}

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn main() -> ! {
    static INITED: AtomicBool = AtomicBool::new(false);

    if cpuid() == 0 {
        unsafe {
            kernel_builder_unchecked_pin().init();
        }
        INITED.store(true, Ordering::Release);
    } else {
        while !INITED.load(Ordering::Acquire) {
            ::core::hint::spin_loop();
        }
        unsafe {
            kernel_ref(|kctx| kctx.inithart());
        }
    }

    unsafe { kernel_ref(|kctx| kctx.scheduler()) }
}
