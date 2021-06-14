use core::pin::Pin;

use pin_project::pin_project;

use crate::{
    arch::memlayout::UART0,
    console::{Console, Printer},
    cpu::Cpus,
    kalloc::Kmem,
    lock::{SleepableLock, SpinLock},
    virtio::VirtioDisk,
};

static mut HAL: Hal = unsafe { Hal::new() };

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
    const unsafe fn new() -> Self {
        Self {
            console: unsafe { Console::new(UART0) },
            printer: Printer::new(),
            kmem: SpinLock::new("KMEM", unsafe { Kmem::new() }),
            cpus: Cpus::new(),
            disk: SleepableLock::new("DISK", unsafe { VirtioDisk::new() }),
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

    pub fn cpus(&self) -> &Cpus {
        &self.cpus
    }

    pub fn disk(self: Pin<&Self>) -> Pin<&SleepableLock<VirtioDisk>> {
        // SAFETY: `HAL` is never moved inside this module, and only shared references are exposed.
        unsafe { Pin::new_unchecked(&self.get_ref().disk) }
    }
}
