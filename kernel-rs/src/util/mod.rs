//! Utilities.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

pub mod branded;
pub mod etrace;
pub mod intrusive_list;
pub mod pinned_array;
pub mod static_arc;
pub mod strong_pin;

pub fn spin_loop() -> ! {
    loop {
        ::core::hint::spin_loop();
    }
}

pub fn memmove(dst: &mut [u8], src: &[u8]) {
    assert_eq!(dst.len(), src.len());

    fn aux<T: Copy>(dst: &mut [u8], src: &[u8]) -> bool {
        let a = core::mem::align_of::<T>();
        let b = dst.as_ptr().align_offset(a) == src.as_ptr().align_offset(a);
        if b {
            let (dpre, dshort, dsuf) = unsafe { dst.align_to_mut::<T>() };
            let (spre, sshort, ssuf) = unsafe { src.align_to::<T>() };
            dpre.copy_from_slice(spre);
            for (d, s) in dshort.iter_mut().zip(sshort) {
                *d = *s;
            }
            dsuf.copy_from_slice(ssuf);
        }
        b
    }

    // Try 8B-move.
    if aux::<u64>(dst, src) {
        return;
    }
    // If 8B-move is not possible, try 4B-move.
    if aux::<u32>(dst, src) {
        return;
    }
    // If 4B-move is not possible, try 2B-move.
    if aux::<u16>(dst, src) {
        return;
    }
    // If 2B-move is not possible, do 1B-move.
    dst.copy_from_slice(src);
}

/// # SAFETY
///
/// Filling a value of `T` with a value of `S` must not break the invariant of `T`.
pub unsafe fn memset<T, S: Copy>(dst: &mut T, v: S)
where
	[u8; core::mem::size_of::<T>() % core::mem::size_of::<S>() + usize::MAX]:,  // We need mem::size_of::<T>() % mem::size_of::<S>() == 0
    [u8; core::mem::align_of::<T>() % core::mem::align_of::<S>() + usize::MAX]:,// We need mem::align_of::<T>() % mem::align_of::<S>() == 0
{
    // SAFETY: T's size/alignment is a multiple of S's size/alignment.
    let buf = unsafe {
        core::slice::from_raw_parts_mut(
            dst as *mut _ as *mut S,
            core::mem::size_of::<T>() / core::mem::size_of::<S>(),
        )
    };
    buf.fill(v);
}
