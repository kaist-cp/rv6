use core::pin::Pin;

use pin_project::pin_project;

use crate::{
    arch::memlayout::UART0,
    console::{Console, Printer},
    cpu::Cpus,
    kalloc::Kmem,
    lock::Spinlock,
    println,
};

static mut HAL: Hal = Hal::new();

pub unsafe fn hal<'s>() -> &'s Hal {
    unsafe { &HAL }
}

/// Returns a pinned mutable reference to the `HAL`.
///
/// # Safety
///
/// The caller should make sure not to call this function multiple times.
/// All mutable accesses to the `HAL` must be done through this.
#[inline]
pub unsafe fn hal_unchecked_pin() -> Pin<&'static mut Hal> {
    // SAFETY: safe if all mutable accesses to the `Hal` are done through this.
    unsafe { Pin::new_unchecked(&mut HAL) }
}

/// Hardware Abstraction Layer
#[pin_project]
pub struct Hal {
    /// Sleeps waiting for there are some input in console buffer.
    pub console: Console,

    pub printer: Spinlock<Printer>,

    #[pin]
    pub kmem: Spinlock<Kmem>,

    pub cpus: Cpus,
}

impl Hal {
    const fn new() -> Self {
        Self {
            console: unsafe { Console::new(UART0) },
            printer: Spinlock::new("PRINTLN", Printer::new()),
            kmem: Spinlock::new("KMEM", unsafe { Kmem::new() }),
            cpus: Cpus::new(),
        }
    }

    /// Initializes the HAL.
    ///
    /// # Safety
    ///
    /// This method should be called only once by the hart 0.
    pub unsafe fn init(self: Pin<&mut Self>) {
        let mut this = self.project();

        // Console.
        this.console.init();

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        unsafe { this.kmem.as_mut().get_pin_mut().init() };
    }
}
