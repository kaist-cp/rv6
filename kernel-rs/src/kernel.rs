use core::fmt::{self, Write};
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};

use pin_project::pin_project;

use crate::util::strong_pin::StrongPin;
use crate::{
    arch::interface::{Arch, TrapManager},
    arch::intr::{intr_init, intr_init_core},
    arch::TargetArch,
    bio::Bcache,
    console::{console_read, console_write},
    cpu::cpuid,
    file::{Devsw, FileTable},
    fs::{DefaultFs, FileSystem},
    hal::{hal, hal_init},
    kalloc::Kmem,
    lock::{SleepableLock, SpinLock},
    param::NDEV,
    proc::Procs,
    util::{branded::Branded, spin_loop},
    vm::KernelMemory,
};

const CONSOLE_IN_DEVSW: usize = 1;

/// The kernel.
static mut KERNEL: Kernel<TargetArch> = unsafe { Kernel::new() };

/// Returns a shared reference to the `KERNEL`.
#[inline]
fn kernel<'s>() -> StrongPin<'s, Kernel<TargetArch>> {
    // SAFETY: there is no way to make a mutable reference to `KERNEL` except calling
    // `kernel_builder_unchecked_pin`, which is unsafe.
    unsafe { StrongPin::new_unchecked(&KERNEL) }
}

/// Creates a `KernelRef` that has a unique, invariant `'id` and points to the `Kernel`.
/// The `KernelRef` can be used only inside the given closure.
///
/// # Safety
///
/// Use this only after the `Kernel` is initialized.
pub unsafe fn kernel_ref<'s, F: for<'new_id> FnOnce(KernelRef<'new_id, 's>) -> R, R>(f: F) -> R {
    Branded::new(kernel(), |k| f(KernelRef(k)))
}

/// Returns a pinned mutable reference to the `KERNEL`.
///
/// # Safety
///
/// There must be no other references to `KERNEL` while the returned reference is alive.
#[inline]
unsafe fn kernel_mut_unchecked<'s>() -> Pin<&'s mut Kernel<TargetArch>> {
    // SAFETY: there are no other references to `KERNEL` while the returned reference is alive.
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
pub struct Kernel<A: Arch> {
    panicked: AtomicBool,

    /// The kernel's memory manager.
    memory: MaybeUninit<KernelMemory<A>>,

    ticks: SleepableLock<u32>,

    /// Current process system.
    #[pin]
    procs: Procs,

    #[pin]
    bcache: Bcache,

    devsw: [Devsw; NDEV],

    #[pin]
    ftable: FileTable,

    #[pin]
    file_system: DefaultFs,
}

/// A branded reference to a `Kernel`.
///
/// # Safety
///
/// The `'id` is always different between different `Kernel` instances.
#[derive(Clone, Copy)]
pub struct KernelRef<'id, 's>(Branded<'id, StrongPin<'s, Kernel<TargetArch>>>);

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

    pub fn as_ref(&self) -> Pin<&Kernel<TargetArch>> {
        self.0.into_inner().as_pin()
    }

    /// Returns a reference to the kernel's ticks.
    pub fn ticks(&self) -> &'s SleepableLock<u32> {
        &self.0.as_pin().get_ref().ticks
    }

    pub fn ps(&self) -> Pin<&'s Procs> {
        unsafe { Pin::new_unchecked(&self.0.as_pin().get_ref().procs) }
    }

    pub fn bcache(&self) -> StrongPin<'s, Bcache> {
        unsafe { StrongPin::new_unchecked(&self.0.as_pin().get_ref().bcache) }
    }

    /// Returns a reference to the kernel's `Devsw` array.
    pub fn devsw(&self) -> &'s [Devsw; NDEV] {
        &self.0.as_pin().get_ref().devsw
    }

    /// Returns a reference to the kernel's `FileSystem`.
    pub fn fs(&self) -> StrongPin<'s, DefaultFs> {
        unsafe { StrongPin::new_unchecked(&self.0.as_pin().get_ref().file_system) }
    }

    pub fn ftable(&self) -> StrongPin<'s, FileTable> {
        unsafe { StrongPin::new_unchecked(&self.0.as_pin().get_ref().ftable) }
    }
}

impl<'id, 's> Deref for KernelRef<'id, 's> {
    type Target = Kernel<TargetArch>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Kernel<TargetArch> {
    /// # Safety
    ///
    /// Must be used only after initializing it with `Kernel::init`.
    const unsafe fn new() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            memory: MaybeUninit::uninit(),
            ticks: SleepableLock::new("time", 0),
            procs: Procs::new(),
            bcache: unsafe { Bcache::new_bcache() },
            devsw: [Devsw {
                read: None,
                write: None,
            }; NDEV],
            ftable: FileTable::new_ftable(),
            file_system: DefaultFs::new(),
        }
    }

    /// Initializes the kernel.
    ///
    /// # Safety
    ///
    /// This method should be called only once by the core 0.
    unsafe fn init(self: Pin<&mut Self>, allocator: Pin<&SpinLock<Kmem>>) {
        self.as_ref().write_str("\nrv6 kernel is booting\n\n");

        let mut this = self.project();

        // Connect read and write system calls to consoleread and consolewrite.
        this.devsw[CONSOLE_IN_DEVSW] = Devsw {
            read: Some(console_read),
            write: Some(console_write),
        };

        // Create kernel memory manager.
        let memory = KernelMemory::new(allocator).expect("PageTable::new failed");

        // Turn on paging.
        // SAFETY: `memory.page_table` contains base address for a valid kernel page table.
        unsafe { this.memory.write(memory).init_register() };

        // Process system.
        this.procs.as_mut().init();

        // Trap vectors.
        TargetArch::trap_init();

        // Install kernel trap vector.
        // SAFETY: It is called first time on this core.
        unsafe { TargetArch::trap_init_core() };

        // Set up interrupt controller.
        // SAFETY: It is only called on core 0 once.
        unsafe { intr_init() };

        // Ask PLIC for device interrupts.
        // SAFETY: It is called first time on this core.
        unsafe { intr_init_core() };

        // Buffer cache.
        this.bcache.init();

        // First user process.
        let fs = unsafe { StrongPin::new_unchecked(this.file_system.as_ref().get_ref()) };
        this.procs.user_proc_init(fs.root(), allocator);
    }

    /// Initializes the kernel for a core.
    ///
    /// # Safety
    ///
    /// This method should be called only once by each core.
    unsafe fn initcore(self: Pin<&Self>) {
        self.write_fmt(format_args!("core {} starting\n", cpuid()));

        // Turn on paging.
        // SAFETY: `self.memory.page_table` contains base address for a valid kernel page table.
        unsafe { self.memory.assume_init_ref().init_register() };

        // Install kernel trap vector.
        // SAFETY: It is called first time on this core.
        unsafe { TargetArch::trap_init_core() };

        // Ask PLIC for device interrupts.
        // SAFETY: It is called first time on this core.
        unsafe { intr_init_core() };
    }

    fn panic(self: Pin<&Self>) {
        self.panicked.store(true, Ordering::Release);
    }

    pub fn is_panicked(self: Pin<&Self>) -> bool {
        self.panicked.load(Ordering::Acquire)
    }

    /// Prints the given formatted string with the Printer.
    pub fn write_fmt(self: Pin<&Self>, args: fmt::Arguments<'_>) {
        let mut guard = if self.is_panicked() {
            hal().get_ref().printer().without_lock(self)
        } else {
            hal().get_ref().printer().lock(self)
        };
        let _ = guard.write_fmt(args);
    }

    pub fn write_str(self: Pin<&Self>, s: &str) {
        self.write_fmt(format_args!("{}", s));
    }
}

/// Handles panic by freezing other CPUs.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    let kernel = kernel().as_pin();
    kernel.panic();
    kernel.write_fmt(format_args!("{}\n", info));

    spin_loop()
}

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn main() -> ! {
    static INITED: AtomicBool = AtomicBool::new(false);

    if cpuid() == 0 {
        unsafe {
            hal_init();
        }
        unsafe {
            kernel_mut_unchecked().init(hal().kmem());
        }
        INITED.store(true, Ordering::Release);
    } else {
        while !INITED.load(Ordering::Acquire) {
            ::core::hint::spin_loop();
        }
        unsafe {
            kernel().as_pin().initcore();
        }
    }

    unsafe { kernel_ref(|kctx| kctx.scheduler()) }
}
