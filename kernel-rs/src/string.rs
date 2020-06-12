use crate::libc;
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
#[no_mangle]
pub unsafe extern "C" fn memset(mut dst: *mut libc::c_void,
                                mut c: libc::c_int, mut n: uint)
 -> *mut libc::c_void {
    let mut cdst: *mut libc::c_char = dst as *mut libc::c_char;
    let mut i: libc::c_int = 0;
    i = 0 as libc::c_int;
    while (i as libc::c_uint) < n {
        *cdst.offset(i as isize) = c as libc::c_char;
        i += 1
    }
    return dst;
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(mut v1: *const libc::c_void,
                                mut v2: *const libc::c_void, mut n: uint)
 -> libc::c_int {
    let mut s1: *const uchar = 0 as *const uchar;
    let mut s2: *const uchar = 0 as *const uchar;
    s1 = v1 as *const uchar;
    s2 = v2 as *const uchar;
    loop  {
        let fresh0 = n;
        n = n.wrapping_sub(1);
        if !(fresh0 > 0 as libc::c_int as libc::c_uint) { break ; }
        if *s1 as libc::c_int != *s2 as libc::c_int {
            return *s1 as libc::c_int - *s2 as libc::c_int
        }
        s1 = s1.offset(1);
        s2 = s2.offset(1)
    }
    return 0 as libc::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn memmove(mut dst: *mut libc::c_void,
                                 mut src: *const libc::c_void, mut n: uint)
 -> *mut libc::c_void {
    let mut s: *const libc::c_char = 0 as *const libc::c_char;
    let mut d: *mut libc::c_char = 0 as *mut libc::c_char;
    s = src as *const libc::c_char;
    d = dst as *mut libc::c_char;
    if s < d && s.offset(n as isize) > d {
        s = s.offset(n as isize);
        d = d.offset(n as isize);
        loop  {
            let fresh1 = n;
            n = n.wrapping_sub(1);
            if !(fresh1 > 0 as libc::c_int as libc::c_uint) { break ; }
            s = s.offset(-1);
            d = d.offset(-1);
            *d = *s
        }
    } else {
        loop  {
            let fresh2 = n;
            n = n.wrapping_sub(1);
            if !(fresh2 > 0 as libc::c_int as libc::c_uint) { break ; }
            let fresh3 = s;
            s = s.offset(1);
            let fresh4 = d;
            d = d.offset(1);
            *fresh4 = *fresh3
        }
    }
    return dst;
}
// memcpy exists to placate GCC.  Use memmove.
#[no_mangle]
pub unsafe extern "C" fn memcpy(mut dst: *mut libc::c_void,
                                mut src: *const libc::c_void, mut n: uint)
 -> *mut libc::c_void {
    return memmove(dst, src, n);
}
#[no_mangle]
pub unsafe extern "C" fn strncmp(mut p: *const libc::c_char,
                                 mut q: *const libc::c_char, mut n: uint)
 -> libc::c_int {
    while n > 0 as libc::c_int as libc::c_uint && *p as libc::c_int != 0 &&
              *p as libc::c_int == *q as libc::c_int {
        n = n.wrapping_sub(1);
        p = p.offset(1);
        q = q.offset(1)
    }
    if n == 0 as libc::c_int as libc::c_uint { return 0 as libc::c_int }
    return *p as uchar as libc::c_int - *q as uchar as libc::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn strncpy(mut s: *mut libc::c_char,
                                 mut t: *const libc::c_char,
                                 mut n: libc::c_int) -> *mut libc::c_char {
    let mut os: *mut libc::c_char = 0 as *mut libc::c_char;
    os = s;
    loop  {
        let fresh5 = n;
        n = n - 1;
        if !(fresh5 > 0 as libc::c_int &&
                 {
                     let fresh6 = t;
                     t = t.offset(1);
                     let fresh7 = s;
                     s = s.offset(1);
                     *fresh7 = *fresh6;
                     (*fresh7 as libc::c_int) != 0 as libc::c_int
                 }) {
            break ;
        }
    }
    loop  {
        let fresh8 = n;
        n = n - 1;
        if !(fresh8 > 0 as libc::c_int) { break ; }
        let fresh9 = s;
        s = s.offset(1);
        *fresh9 = 0 as libc::c_int as libc::c_char
    }
    return os;
}
// Like strncpy but guaranteed to NUL-terminate.
#[no_mangle]
pub unsafe extern "C" fn safestrcpy(mut s: *mut libc::c_char,
                                    mut t: *const libc::c_char,
                                    mut n: libc::c_int) -> *mut libc::c_char {
    let mut os: *mut libc::c_char = 0 as *mut libc::c_char;
    os = s;
    if n <= 0 as libc::c_int { return os }
    loop  {
        n -= 1;
        if !(n > 0 as libc::c_int &&
                 {
                     let fresh10 = t;
                     t = t.offset(1);
                     let fresh11 = s;
                     s = s.offset(1);
                     *fresh11 = *fresh10;
                     (*fresh11 as libc::c_int) != 0 as libc::c_int
                 }) {
            break ;
        }
    }
    *s = 0 as libc::c_int as libc::c_char;
    return os;
}
#[no_mangle]
pub unsafe extern "C" fn strlen(mut s: *const libc::c_char) -> libc::c_int {
    let mut n: libc::c_int = 0;
    n = 0 as libc::c_int;
    while *s.offset(n as isize) != 0 { n += 1 }
    return n;
}
