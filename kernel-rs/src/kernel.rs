use core::fmt::{self, Write};
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    bio::{Bcache, BcacheInner},
    console::{consoleinit, Console, Printer},
    file::{Devsw, FileTable},
    fs::{FileSystem, Itable},
    kalloc::Kmem,
    page::Page,
    param::{NCPU, NDEV},
    plic::{plicinit, plicinithart},
    println,
    proc::{cpuid, procinit, scheduler, Cpu, ProcessSystem},
    sleepablelock::Sleepablelock,
    spinlock::Spinlock,
    trap::{trapinit, trapinithart},
    uart::Uart,
    vm::KernelMemory,
};

/// The kernel.
static mut KERNEL: Kernel = Kernel::zero();

/// After intialized, the kernel is safe to immutably access.
#[inline]
pub fn kernel() -> &'static Kernel {
    unsafe { &KERNEL }
}

/// # Safety
///
/// The `Kernel` never moves `_bcache_inner` and only provides a
/// pinned mutable reference of it to the outside.
pub struct Kernel {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Sleepablelock<Console>,

    /// TODO(https://github.com/kaist-cp/rv6/issues/298): Kernel owns uart temporarily.
    /// This might be changed after refactoring relationship between Console-Uart-Printer.
    pub uart: Uart,

    pub printer: Spinlock<Printer>,

    kmem: Spinlock<Kmem>,

    /// The kernel's memory manager.
    memory: MaybeUninit<KernelMemory>,

    pub ticks: Sleepablelock<u32>,

    /// Current process system.
    pub procs: ProcessSystem,

    cpus: [Cpu; NCPU],

    _bcache_inner: BcacheInner, // Never access this after initialization.
    bcache: MaybeUninit<Bcache>,

    pub devsw: [Devsw; NDEV],

    pub ftable: FileTable,

    pub itable: Itable,

    pub file_system: FileSystem,
}

impl Kernel {
    const fn zero() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            console: Sleepablelock::new("CONS", Console::new()),
            uart: Uart::new(),
            printer: Spinlock::new("PRINTLN", Printer::new()),
            kmem: Spinlock::new("KMEM", Kmem::new()),
            memory: MaybeUninit::uninit(),
            ticks: Sleepablelock::new("time", 0),
            procs: ProcessSystem::zero(),
            cpus: [Cpu::new(); NCPU],
            // Safe since we never move `_bcache_inner` and only provide a
            // pinned mutable reference of it to the outside.
            _bcache_inner: unsafe { BcacheInner::zero() },
            bcache: MaybeUninit::uninit(),
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

    /// Free the page of physical memory pointed at by v,
    /// which normally should have been returned by a
    /// call to kernel().alloc().  (The exception is when
    /// initializing the allocator; see Kmem::init.)
    pub fn free(&self, mut page: Page) {
        // Fill with junk to catch dangling refs.
        page.write_bytes(1);

        kernel().kmem.lock().free(page);
    }

    /// Allocate one 4096-byte page of physical memory.
    /// Returns a pointer that the kernel can use.
    /// Returns None if the memory cannot be allocated.
    pub fn alloc(&self) -> Option<Page> {
        let mut page = kernel().kmem.lock().alloc()?;

        // fill with junk
        page.write_bytes(5);
        Some(page)
    }

    /// Prints the given formatted string with the Printer.
    pub fn printer_write_fmt(&self, args: fmt::Arguments<'_>) -> fmt::Result {
        if self.is_panicked() {
            unsafe { kernel().printer.get_mut_unchecked().write_fmt(args) }
        } else {
            let mut lock = kernel().printer.lock();
            lock.write_fmt(args)
        }
    }

    /// Return this CPU's cpu struct.
    ///
    /// It is safe to call this function with interrupts enabled, but returned address may not be the
    /// current CPU since the scheduler can move the process to another CPU on time interrupt.
    pub fn mycpu(&self) -> *mut Cpu {
        let id: usize = cpuid();
        &self.cpus[id] as *const _ as *mut _
    }

    /// # Safety
    ///
    /// Use only after `kernel_main()`, which is where `bcache` gets initialized.
    pub fn get_bcache(&self) -> &Bcache {
        unsafe { self.bcache.assume_init_ref() }
    }
}

/// print! macro prints to the console using printer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::kernel::kernel().printer_write_fmt(format_args!($($arg)*)).unwrap();
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
    kernel().panic();
    println!("{}", info);

    crate::utils::spin_loop()
}

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn kernel_main() -> ! {
    static STARTED: AtomicBool = AtomicBool::new(false);

    if cpuid() == 0 {
        // Initialize the kernel.

        // Console.
        Uart::init();
        unsafe { consoleinit(&mut KERNEL.devsw) };

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        unsafe { KERNEL.kmem.get_mut().init() };

        // Create kernel memory manager.
        let memory = KernelMemory::new().expect("PageTable::new failed");

        // Turn on paging.
        unsafe { KERNEL.memory.write(memory).init_hart() };

        // Process system.
        unsafe { procinit(&mut KERNEL.procs) };

        // Trap vectors.
        unsafe { trapinit() };

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Set up interrupt controller.
        unsafe { plicinit() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };

        // Buffer cache.
        unsafe {
            KERNEL.bcache = MaybeUninit::new(Spinlock::new(
                "BCACHE",
                Pin::new_unchecked(&mut KERNEL._bcache_inner),
            ));
            KERNEL.bcache.assume_init_mut().get_mut().as_mut().init()
        };

        // Emulated hard disk.
        unsafe { KERNEL.file_system.disk.get_mut().init() };

        // First user process.
        unsafe { KERNEL.procs.user_proc_init() };
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            spin_loop();
        }

        println!("hart {} starting", cpuid());

        // Turn on paging.
        unsafe { KERNEL.memory.assume_init_mut().init_hart() };

        // Install kernel trap vector.
        unsafe { trapinithart() };

        // Ask PLIC for device interrupts.
        unsafe { plicinithart() };
    }

    unsafe { scheduler() }
}
