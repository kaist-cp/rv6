use core::sync::atomic::{spin_loop_hint, AtomicBool, Ordering};
use core::{
    fmt::{self, Write},
    ptr,
};
use spin::Once;

use crate::{
    bio::Bcache,
    console::{consoleinit, Console},
    file::{Devsw, File, Inode},
    fs::FileSystem,
    kalloc::{end, kinit, Kmem},
    memlayout::PHYSTOP,
    page::Page,
    param::{NCPU, NDEV, NFILE, NINODE},
    plic::{plicinit, plicinithart},
    pool::RcPool,
    println,
    proc::{cpuid, procinit, scheduler, Cpu, ProcessSystem},
    riscv::PGSIZE,
    sleepablelock::Sleepablelock,
    spinlock::Spinlock,
    trap::{trapinit, trapinithart},
    uart::Uart,
    virtio_disk::{virtio_disk_init, Disk},
    vm::{kvminit, kvminithart, PageTable},
};

/// The kernel.
static mut KERNEL: Kernel = Kernel::zero();

/// After intialized, the kernel is safe to immutably access.
pub fn kernel() -> &'static Kernel {
    unsafe { &KERNEL }
}

pub struct Kernel {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Sleepablelock<Console>,

    kmem: Spinlock<Kmem>,

    /// The kernel's page table.
    pub page_table: PageTable,

    pub ticks: Sleepablelock<u32>,

    /// Current process system.
    pub procs: ProcessSystem,

    cpus: [Cpu; NCPU],

    pub bcache: Spinlock<Bcache>,

    /// Memory for virtio descriptors `&c` for queue 0.
    ///
    /// This is a global instead of allocated because it must be multiple contiguous pages, which
    /// `kernel().alloc()` doesn't support, and page aligned.
    // TODO(efenniht): I moved out pages from Disk. Did I changed semantics (pointer indirection?)
    virtqueue: [Page; 2],

    /// It may sleep until some Descriptors are freed.
    pub disk: Sleepablelock<Disk>,

    pub devsw: [Devsw; NDEV],

    pub ftable: Spinlock<RcPool<File, NFILE>>,

    pub icache: Spinlock<[Inode; NINODE]>,

    pub file_system: Once<FileSystem>,
}

impl Kernel {
    const fn zero() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            console: Sleepablelock::new("CONS", Console::new()),
            kmem: Spinlock::new("KMEM", Kmem::new()),
            page_table: PageTable::zero(),
            ticks: Sleepablelock::new("time", 0),
            procs: ProcessSystem::zero(),
            cpus: [Cpu::new(); NCPU],
            bcache: Spinlock::new("BCACHE", Bcache::zero()),
            virtqueue: [Page::DEFAULT, Page::DEFAULT],
            disk: Sleepablelock::new("virtio_disk", Disk::zero()),
            devsw: [Devsw {
                read: None,
                write: None,
            }; NDEV],
            ftable: Spinlock::new("FTABLE", RcPool::new()),
            icache: Spinlock::new("ICACHE", [Inode::zero(); NINODE]),
            file_system: Once::new(),
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
    /// initializing the allocator; see kinit above.)
    pub unsafe fn free(&self, pa: *mut u8) {
        if (pa as usize).wrapping_rem(PGSIZE) != 0
            || pa < end.as_mut_ptr()
            || pa as usize >= PHYSTOP
        {
            panic!("Kernel::free");
        }

        // Fill with junk to catch dangling refs.
        ptr::write_bytes(pa, 1, PGSIZE);

        kernel().kmem.lock().free(pa);
    }

    /// Allocate one 4096-byte page of physical memory.
    /// Returns a pointer that the kernel can use.
    /// Returns 0 if the memory cannot be allocated.
    pub unsafe fn alloc(&self) -> *mut u8 {
        let ret = kernel().kmem.lock().alloc();
        if ret.is_null() {
            return ret;
        }

        // fill with junk
        ptr::write_bytes(ret, 5, PGSIZE);
        ret
    }

    /// Prints the given formatted string with the Console.
    pub fn console_write_fmt(&self, args: fmt::Arguments<'_>) -> fmt::Result {
        if self.is_panicked() {
            unsafe { kernel().console.get_mut_unchecked().write_fmt(args) }
        } else {
            let mut lock = kernel().console.lock();
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

/// print! macro prints to the console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::kernel::kernel().console_write_fmt(format_args!($($arg)*)).unwrap();
    };
}

/// println! macro prints to the console.
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
        kinit(KERNEL.kmem.get_mut());

        // Create kernel page table.
        kvminit(&mut KERNEL.page_table);

        // Turn on paging.
        kvminithart(&kernel().page_table);

        // Process system.
        procinit(&mut KERNEL.procs, &mut KERNEL.page_table);

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
        virtio_disk_init(&mut KERNEL.virtqueue, KERNEL.disk.get_mut());

        // First user process.
        KERNEL.procs.user_proc_init();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            spin_loop_hint();
        }

        println!("hart {} starting", cpuid());

        // Turn on paging.
        kvminithart(&kernel().page_table);

        // Install kernel trap vector.
        trapinithart();

        // Ask PLIC for device interrupts.
        plicinithart();
    }

    scheduler()
}
