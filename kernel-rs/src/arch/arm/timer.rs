use core::time::Duration;

use cortex_a::{asm::barrier, registers::*};
use tock_registers::interfaces::{ReadWriteable, Readable, Writeable};

use crate::{kernel::KernelRef, timer::TimeManager};

const NS_PER_S: u64 = 1_000_000_000;

pub struct Timer;

impl TimeManager for Timer {
    /// The uptime since power-on of the device.
    ///
    /// This includes time consumed by firmware and bootloaders.
    fn uptime<'id, 's>(_kernel: KernelRef<'id, 's>) -> Result<usize, ()> {
        let current_count: u64 = Self::read_cntpct() * NS_PER_S;
        let frq: u64 = CNTFRQ_EL0.get();

        Ok((current_count / frq) as usize)
    }

    /// Spin for a given duration.
    fn spin_for<'id, 's>(kernel: &KernelCtx<'id, 's>, duration: usize) -> Result<(), ()> {
        // Instantly return on zero.
        if duration == 0 {
            return Ok(());
        }

        // Calculate the register compare value.
        let frq = CNTFRQ_EL0.get();
        let x = match frq.checked_mul(duration as u64) {
            None => {
                kernel
                    .as_ref()
                    .write_str("Spin duration too long, skipping");
                return Err(());
            }
            Some(val) => val,
        };
        let tval = x / NS_PER_S;

        // Check if it is within supported bounds.
        let warn: Option<&str> = if tval == 0 {
            Some("smaller")
        // The upper 32 bits of CNTP_TVAL_EL0 are reserved.
        } else if tval > u32::MAX.into() {
            Some("bigger")
        } else {
            None
        };

        if let Some(w) = warn {
            kernel.as_ref().write_fmt(format_args!(
                "Spin duration {} than architecturally supported, skipping",
                w
            ));
            return Err(());
        }

        // Set the compare value register.
        CNTP_TVAL_EL0.set(tval);

        // Kick off the counting.                       // Disable timer interrupt.
        CNTP_CTL_EL0.modify(CNTP_CTL_EL0::ENABLE::SET + CNTP_CTL_EL0::IMASK::SET);

        // ISTATUS will be '1' when cval ticks have passed. Busy-check it.
        while !CNTP_CTL_EL0.matches_all(CNTP_CTL_EL0::ISTATUS::SET) {}

        // Disable counting again.
        CNTP_CTL_EL0.modify(CNTP_CTL_EL0::ENABLE::CLEAR);
        Ok(())
    }
}

impl Timer {
    /// The timer's resolution.
    pub fn resolution() -> Duration {
        Duration::from_nanos(NS_PER_S / CNTFRQ_EL0.get())
    }

    fn read_cntpct() -> u64 {
        // Prevent that the counter is read ahead of time due to out-of-order execution.
        unsafe { barrier::isb(barrier::SY) };
        CNTPCT_EL0.get()
    }
}
