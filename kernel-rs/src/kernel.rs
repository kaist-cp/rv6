use core::fmt::{self, Write};
use core::sync::atomic::{spin_loop_hint, AtomicBool, Ordering};

use crate::{
    bio::Bcache,
    console::{consoleinit, Console, Printer},
    file::{Devsw, FileTable},
    fs::{FileSystem, Itable},
    kalloc::{end, Kmem},
    memlayout::PHYSTOP,
    page::{Page, RawPage},
    param::{NCPU, NDEV},
    plic::{plicinit, plicinithart},
    println,
    proc::{cpuid, procinit, scheduler, Cpu, ProcessSystem},
    riscv::PGSIZE,
    sleepablelock::Sleepablelock,
    spinlock::Spinlock,
    trap::{trapinit, trapinithart},
    uart::Uart,
    virtio_disk::virtio_disk_init,
    vm::{KVAddr, PageTable},
};

/// The kernel.
// TODO(rv6): remove pub from `pub static mut KERNEL`.
pub static mut KERNEL: Kernel = Kernel::zero();

/// After intialized, the kernel is safe to immutably access.
#[inline]
pub fn kernel() -> &'static Kernel {
    unsafe { &KERNEL }
}

pub struct Kernel {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Sleepablelock<Console>,

    /// TODO(@coolofficials): Kernel owns uart temporarily.
    /// This might be changed after refactoring relationship between Console-Uart-Printer.
    pub uart: Uart,

    pub printer: Spinlock<Printer>,

    kmem: Spinlock<Kmem>,

    /// The kernel's page table.
    pub page_table: PageTable<KVAddr>,

    pub ticks: Sleepablelock<u32>,

    /// Current process system.
    pub procs: ProcessSystem,

    cpus: [Cpu; NCPU],

    pub bcache: Bcache,

    /// Memory for virtio descriptors `&c` for queue 0.
    ///
    // TODO(efenniht): I moved out pages from Disk. Did I changed semantics (pointer indirection?)
    virtqueue: [RawPage; 2],

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
            page_table: unsafe { PageTable::zero() },
            ticks: Sleepablelock::new("time", 0),
            procs: ProcessSystem::zero(),
            cpus: [Cpu::new(); NCPU],
            bcache: Bcache::zero(),
            virtqueue: [RawPage::DEFAULT, RawPage::DEFAULT],
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
        let pa = page.addr().into_usize();
        debug_assert!(
            pa % PGSIZE == 0 && (pa as *mut _) >= unsafe { end.as_mut_ptr() } && pa < PHYSTOP,
            "[Kernel::free]"
        );

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
        consoleinit(&mut KERNEL.devsw);

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        KERNEL.kmem.get_mut().init();

        // Create kernel page table.
        KERNEL.page_table = PageTable::<KVAddr>::new().expect("PageTable::new failed");

        // Turn on paging.
        KERNEL.page_table.init_hart();

        // Process system.
        procinit(&mut KERNEL.procs);

        // Trap vectors.
        trapinit();

        // Install kernel trap vector.
        trapinithart();

        // Set up interrupt controller.
        plicinit();

        // Ask PLIC for device interrupts.
        plicinithart();

        // Buffer cache.
        KERNEL.bcache.get_mut().init();

        // Emulated hard disk.
        virtio_disk_init(&mut KERNEL.virtqueue, KERNEL.file_system.disk.get_mut());

        // First user process.
        KERNEL.procs.user_proc_init();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            spin_loop_hint();
        }

        println!("hart {} starting", cpuid());

        // Turn on paging.
        KERNEL.page_table.init_hart();

        // Install kernel trap vector.
        trapinithart();

        // Ask PLIC for device interrupts.
        plicinithart();
    }

    scheduler()
}
