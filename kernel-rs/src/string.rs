use crate::libc;

pub unsafe fn strncmp(mut p: *const libc::CChar, mut q: *const libc::CChar, mut n: u32) -> i32 {
    while n > 0 && *p as i32 != 0 && *p as i32 == *q as i32 {
        n = n.wrapping_sub(1);
        p = p.offset(1);
        q = q.offset(1)
    }
    if n == 0 {
        return 0;
    }
    *p as u8 as i32 - *q as u8 as i32
}

pub unsafe fn strncpy(
    mut s: *mut libc::CChar,
    mut t: *const libc::CChar,
    mut n: i32,
) -> *mut libc::CChar {
    let os = s;
    loop {
        let fresh5 = n;
        n -= 1;
        if !(fresh5 > 0 && {
            let fresh6 = t;
            t = t.offset(1);
            let fresh7 = s;
            s = s.offset(1);
            *fresh7 = *fresh6;
            (*fresh7 as i32) != 0
        }) {
            break;
        }
    }
    loop {
        let fresh8 = n;
        n -= 1;
        if fresh8 <= 0 {
            break;
        }
        let fresh9 = s;
        s = s.offset(1);
        *fresh9 = 0 as libc::CChar
    }
    os
}

/// Like strncpy but guaranteed to NUL-terminate.
pub unsafe fn safestrcpy(
    mut s: *mut libc::CChar,
    mut t: *const libc::CChar,
    mut n: i32,
) -> *mut libc::CChar {
    let os = s;
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
            (*fresh11 as i32) != 0
        }) {
            break;
        }
    }
    *s = 0 as libc::CChar;
    os
}

pub unsafe fn strlen(s: *const libc::CChar) -> i32 {
    let mut n: i32 = 0;
    while *s.offset(n as isize) != 0 {
        n += 1
    }
    n
}
