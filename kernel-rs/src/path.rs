use cstr_core::CStr;

#[repr(transparent)]
struct Path(CStr);

impl Path {
    fn new(cstr: &CStr) -> &Self {
        // SAFETY: `&Path` is layout-compatible with `CStr` because of  its attribute
        // `#[repr(transparent)]`.
        unsafe { &*(cstr as *const CStr as *const Self) }
    }
}
