/// Like strncpy but guaranteed to NUL-terminate.
pub unsafe fn safestrcpy(mut s: *mut u8, mut t: *const u8, mut n: i32) -> *mut u8 {
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
    *s = 0;
    os
}

pub unsafe fn strlen(s: *const u8) -> i32 {
    let mut n: i32 = 0;
    while *s.offset(n as isize) != 0 {
        n += 1
    }
    n
}
