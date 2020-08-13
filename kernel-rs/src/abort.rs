/// Causes execution to halt and prevent progress of the current and less privileged software
/// components. This should be triggered when a non-recoverable event is identified which leaves the
/// system in an inconsistent state.
///
/// TODO: Should this also reset the system?
/// TODO(HfO2): This function needs to be weakly linked because some tests have custom `abort`
/// function but still need HfO2. Dividing HfO2 into many libraries may resolve this.
#[cfg(not(feature = "test"))]
pub fn abort() -> ! {
    crate::utils::spin_loop()
}
