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

    if aux::<u64>(dst, src) {
        return;
    }
    if aux::<u32>(dst, src) {
        return;
    }
    if aux::<u16>(dst, src) {
        return;
    }
    dst.copy_from_slice(src);
}
