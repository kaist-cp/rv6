use core::pin::Pin;

use pin_project::pin_project;

use crate::{
    arch::interface::Arch,
    arch::TargetArch,
    bio::Buf,
    console::{Console, Printer},
    cpu::Cpus,
    kalloc::Kmem,
    lock::{new_sleepable_lock, new_spin_lock, SleepableLock, SpinLock},
    page::Page,
    proc::KernelCtx,
    virtio::VirtioDisk,
};

static mut HAL: Hal = unsafe { Hal::new::<TargetArch>() };

pub fn hal<'s>() -> Pin<&'s Hal> {
    // SAFETY: there is no way to make a mutable reference to `HAL` except calling `hal_init`,
    // which is unsafe.
    unsafe { Pin::new_unchecked(&HAL) }
}

/// Initializes `HAL`.
///
/// # Safety
///
/// * There must be no reference to `HAL` while this function is running.
/// * This function must be called only once.
pub unsafe fn hal_init() {
    // SAFETY: there is no reference to `HAL`.
    let hal = unsafe { &mut HAL };
    // SAFETY: we do not move `hal`.
    let hal = unsafe { Pin::new_unchecked(hal) };
    // SAFETY: this function is called only once.
    unsafe { hal.init() };
}

/// Hardware Abstraction Layer
#[pin_project]
pub struct Hal {
    /// Sleeps waiting for there are some input in console buffer.
    console: Console,

    printer: Printer,

    #[pin]
    kmem: SpinLock<Kmem>,

    cpus: Cpus,

    #[pin]
    disk: SleepableLock<VirtioDisk>,
}

impl Hal {
    /// # Safety
    ///
    /// Must be used only after initializing it with `Hal::init`.
    const unsafe fn new<A: Arch>() -> Self {
        Self {
            console: unsafe { Console::new(A::UART0) },
            printer: Printer::new(),
            kmem: new_spin_lock("KMEM", unsafe { Kmem::new() }),
            cpus: Cpus::new(),
            disk: new_sleepable_lock("DISK", unsafe { VirtioDisk::new() }),
        }
    }

    /// Initializes `HAL`.
    ///
    /// # Safety
    ///
    /// This method must be called only once.
    unsafe fn init(self: Pin<&mut Self>) {
        let this = self.project();

        // Console.
        this.console.init();

        // Physical page allocator.
        unsafe { this.kmem.get_pin_mut().init() };

        this.disk.get_pin_mut().as_ref().init();
    }

    pub fn console(&self) -> &Console {
        &self.console
    }

    pub fn printer(&self) -> &Printer {
        &self.printer
    }

    pub fn kmem(self: Pin<&Self>) -> Pin<&SpinLock<Kmem>> {
        // SAFETY: `HAL` is never moved inside this module, and only shared references are exposed.
        unsafe { Pin::new_unchecked(&self.get_ref().kmem) }
    }

    pub fn free(self: Pin<&Self>, mut page: Page) {
        page.write_bytes(1);
        self.kmem().pinned_lock().get_pin_mut().as_ref().free(page);
    }

    pub fn alloc(self: Pin<&Self>, init_value: Option<u8>) -> Option<Page> {
        let mut page = self.kmem().pinned_lock().get_pin_mut().as_ref().alloc()?;

        // fill with junk or received init value
        let init_value = init_value.unwrap_or(5);
        page.write_bytes(init_value);
        Some(page)
    }

    pub fn cpus(&self) -> &Cpus {
        &self.cpus
    }

    pub fn disk(self: Pin<&Self>) -> Pin<&SleepableLock<VirtioDisk>> {
        // SAFETY: `HAL` is never moved inside this module, and only shared references are exposed.
        unsafe { Pin::new_unchecked(&self.get_ref().disk) }
    }

    pub fn read(self: Pin<&Self>, dev: u32, blockno: u32, ctx: &KernelCtx<'_, '_>) -> Buf {
        VirtioDisk::read(self.disk(), dev, blockno, ctx)
    }

    pub fn write(self: Pin<&Self>, buf: &mut Buf, ctx: &KernelCtx<'_, '_>) {
        VirtioDisk::write(self.disk(), buf, ctx)
    }
}
