use core::fmt::{self, Write};
use core::sync::atomic::{spin_loop_hint, AtomicBool, Ordering};
use spin::Once;

use crate::{
    arena::{ArrayArena, ArrayEntry, MruArena, MruEntry},
    bio::BufEntry,
    console::{consoleinit, Console},
    file::{Devsw, File},
    fs::{FileSystem, Inode},
    kalloc::{end, kinit, Kmem},
    memlayout::PHYSTOP,
    page::{Page, RawPage},
    param::{NBUF, NCPU, NDEV, NFILE, NINODE},
    plic::{plicinit, plicinithart},
    println,
    proc::{cpuid, procinit, scheduler, Cpu, ProcessSystem},
    riscv::PGSIZE,
    sleepablelock::Sleepablelock,
    spinlock::Spinlock,
    trap::{trapinit, trapinithart},
    uart::Uart,
    virtio_disk::{virtio_disk_init, Disk},
    vm::{KVAddr, PageTable},
};

/// The kernel.
static mut KERNEL: Kernel = Kernel::zero();

/// After intialized, the kernel is safe to immutably access.
#[inline]
pub fn kernel() -> &'static Kernel {
    unsafe { &KERNEL }
}

pub struct Kernel {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Console,

    kmem: Spinlock<Kmem>,

    /// The kernel's page table.
    pub page_table: PageTable<KVAddr>,

    pub ticks: Sleepablelock<u32>,

    /// Current process system.
    pub procs: ProcessSystem,

    cpus: [Cpu; NCPU],

    pub bcache: Spinlock<MruArena<BufEntry, NBUF>>,

    /// Memory for virtio descriptors `&c` for queue 0.
    ///
    /// This is a global instead of allocated because it must be multiple contiguous pages, which
    /// `kernel().alloc()` doesn't support, and page aligned.
    // TODO(efenniht): I moved out pages from Disk. Did I changed semantics (pointer indirection?)
    virtqueue: [RawPage; 2],

    /// It may sleep until some Descriptors are freed.
    pub disk: Sleepablelock<Disk>,

    pub devsw: [Devsw; NDEV],

    pub ftable: Spinlock<ArrayArena<File, NFILE>>,

    pub icache: Spinlock<ArrayArena<Inode, NINODE>>,

    pub file_system: Once<FileSystem>,
}

// TODO(rv6): ugly tricks with magic numbers. Fix it...

const fn bcache_entry(_: usize) -> MruEntry<BufEntry> {
    MruEntry::new(BufEntry::zero())
}

const fn ftable_entry(_: usize) -> ArrayEntry<File> {
    ArrayEntry::new(File::zero())
}

const fn icache_entry(_: usize) -> ArrayEntry<Inode> {
    ArrayEntry::new(Inode::zero())
}

impl Kernel {
    const fn zero() -> Self {
        Self {
            panicked: AtomicBool::new(false),
            console: Console::new(),
            kmem: Spinlock::new("KMEM", Kmem::new()),
            page_table: PageTable::zero(),
            ticks: Sleepablelock::new("time", 0),
            procs: ProcessSystem::zero(),
            cpus: [Cpu::new(); NCPU],
            bcache: Spinlock::new(
                "BCACHE",
                MruArena::new(array_const_fn_init![bcache_entry; 30]),
            ),
            virtqueue: [RawPage::DEFAULT, RawPage::DEFAULT],
            disk: Sleepablelock::new("virtio_disk", Disk::zero()),
            devsw: [Devsw {
                read: None,
                write: None,
            }; NDEV],
            ftable: Spinlock::new(
                "FTABLE",
                ArrayArena::new(array_const_fn_init![ftable_entry; 100]),
            ),
            icache: Spinlock::new(
                "ICACHE",
                ArrayArena::new(array_const_fn_init![icache_entry; 50]),
            ),
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
    pub unsafe fn free(&self, mut page: Page) {
        let pa = page.addr().into_usize();
        assert!(
            pa.wrapping_rem(PGSIZE) == 0 && (pa as *mut _) >= end.as_mut_ptr() && pa < PHYSTOP,
            "[Kernel::free]"
        );

        // Fill with junk to catch dangling refs.
        page.write_bytes(1);

        kernel().kmem.lock().free(page);
    }

    /// Allocate one 4096-byte page of physical memory.
    /// Returns a pointer that the kernel can use.
    /// Returns 0 if the memory cannot be allocated.
    pub unsafe fn alloc(&self) -> Option<Page> {
        let mut page = kernel().kmem.lock().alloc()?;

        // fill with junk
        page.write_bytes(5);
        Some(page)
    }

    /// Prints the given formatted string with the Console.
    pub fn console_write_fmt(&self, args: fmt::Arguments<'_>) -> fmt::Result {
        if self.is_panicked() {
            unsafe { self.console.printer.get_mut_unchecked().write_fmt(args) }
        } else {
            let mut lock = self.console.printer.lock();
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

    pub fn fsinit(&self, dev: i32) {
        self.file_system.call_once(|| FileSystem::new(dev));
    }

    pub fn fs(&self) -> &FileSystem {
        if let Some(fs) = self.file_system.get() {
            fs
        } else {
            unreachable!()
        }
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
        KERNEL.page_table.kvminit();

        // Turn on paging.
        kernel().page_table.kvminithart();

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
        kernel().page_table.kvminithart();

        // Install kernel trap vector.
        trapinithart();

        // Ask PLIC for device interrupts.
        plicinithart();
    }

    scheduler()
}
