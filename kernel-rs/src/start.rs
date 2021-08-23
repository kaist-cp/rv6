use crate::arch::interface::Arch;
use crate::arch::TargetArch;

/// entry.S jumps here in machine mode on stack0.
///
/// # Safety
///
/// This function must be called from entry.S, and only once.
#[no_mangle]
pub unsafe fn start() {
    unsafe {
        TargetArch::start();
    }
}
