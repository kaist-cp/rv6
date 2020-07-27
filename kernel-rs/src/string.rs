use crate::libc;
use core::ptr;
#[no_mangle]
pub unsafe extern "C" fn memset(
    mut dst: *mut libc::c_void,
    mut c: i32,
    mut n: u32,
) -> *mut libc::c_void {
    let mut cdst: *mut libc::c_char = dst as *mut libc::c_char;
    let mut i: i32 = 0;
    i = 0 as i32;
    while (i as u32) < n {
        *cdst.offset(i as isize) = c as libc::c_char;
        i += 1
    }
    dst
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(
    mut v1: *const libc::c_void,
    mut v2: *const libc::c_void,
    mut n: u32,
) -> i32 {
    let mut s1: *const u8 = ptr::null();
    let mut s2: *const u8 = ptr::null();
    s1 = v1 as *const u8;
    s2 = v2 as *const u8;
    loop {
        let fresh0 = n;
        n = n.wrapping_sub(1);
        if fresh0 <= 0 as u32 {
            break;
        }
        if *s1 as i32 != *s2 as i32 {
            return *s1 as i32 - *s2 as i32;
        }
        s1 = s1.offset(1);
        s2 = s2.offset(1)
    }
    0 as i32
}
#[no_mangle]
pub unsafe extern "C" fn memmove(
    mut dst: *mut libc::c_void,
    mut src: *const libc::c_void,
    mut n: u32,
) -> *mut libc::c_void {
    let mut s: *const libc::c_char = ptr::null();
    let mut d: *mut libc::c_char = ptr::null_mut();
    s = src as *const libc::c_char;
    d = dst as *mut libc::c_char;
    if s < d && s.offset(n as isize) > d {
        s = s.offset(n as isize);
        d = d.offset(n as isize);
        loop {
            let fresh1 = n;
            n = n.wrapping_sub(1);
            if fresh1 <= 0 as i32 as u32 {
                break;
            }
            s = s.offset(-1);
            d = d.offset(-1);
            *d = *s
        }
    } else {
        loop {
            let fresh2 = n;
            n = n.wrapping_sub(1);
            if fresh2 <= 0 as i32 as u32 {
                break;
            }
            let fresh3 = s;
            s = s.offset(1);
            let fresh4 = d;
            d = d.offset(1);
            *fresh4 = *fresh3
        }
    }
    dst
}
/// memcpy exists to placate GCC.  Use memmove.
#[no_mangle]
pub unsafe extern "C" fn memcpy(
    mut dst: *mut libc::c_void,
    mut src: *const libc::c_void,
    mut n: u32,
) -> *mut libc::c_void {
    memmove(dst, src, n)
}
#[no_mangle]
pub unsafe extern "C" fn strncmp(
    mut p: *const libc::c_char,
    mut q: *const libc::c_char,
    mut n: u32,
) -> i32 {
    while n > 0 as u32 && *p as i32 != 0 && *p as i32 == *q as i32 {
        n = n.wrapping_sub(1);
        p = p.offset(1);
        q = q.offset(1)
    }
    if n == 0 as u32 {
        return 0 as i32;
    }
    *p as u8 as i32 - *q as u8 as i32
}
#[no_mangle]
pub unsafe extern "C" fn strncpy(
    mut s: *mut libc::c_char,
    mut t: *const libc::c_char,
    mut n: i32,
) -> *mut libc::c_char {
    let mut os: *mut libc::c_char = ptr::null_mut();
    os = s;
    loop {
        let fresh5 = n;
        n -= 1;
        if !(fresh5 > 0 as i32 && {
            let fresh6 = t;
            t = t.offset(1);
            let fresh7 = s;
            s = s.offset(1);
            *fresh7 = *fresh6;
            (*fresh7 as i32) != 0 as i32
        }) {
            break;
        }
    }
    loop {
        let fresh8 = n;
        n -= 1;
        if fresh8 <= 0 as i32 {
            break;
        }
        let fresh9 = s;
        s = s.offset(1);
        *fresh9 = 0 as i32 as libc::c_char
    }
    os
}
/// Like strncpy but guaranteed to NUL-terminate.
#[no_mangle]
pub unsafe extern "C" fn safestrcpy(
    mut s: *mut libc::c_char,
    mut t: *const libc::c_char,
    mut n: i32,
) -> *mut libc::c_char {
    let mut os: *mut libc::c_char = ptr::null_mut();
    os = s;
    if n <= 0 {
        return os;
    }
    loop {
        n -= 1;
        if !(n > 0 && {
            let fresh10 = t;
            t = t.offset(1);
            let fresh11 = s;
            s = s.offset(1);
            *fresh11 = *fresh10;
            (*fresh11 as i32) != 0 as i32
        }) {
            break;
        }
    }
    *s = 0 as i32 as libc::c_char;
    os
}
#[no_mangle]
pub unsafe extern "C" fn strlen(mut s: *const libc::c_char) -> i32 {
    let mut n: i32 = 0;
    while *s.offset(n as isize) != 0 {
        n += 1
    }
    n
}
