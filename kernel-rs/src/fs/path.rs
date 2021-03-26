use core::cmp;

use cstr_core::CStr;

use super::DIRSIZ;

#[derive(PartialEq)]
#[repr(transparent)]
pub struct FileName {
    // Invariant:
    // - The slice contains no NUL characters.
    // - The slice is not longer than DIRSIZ.
    inner: [u8],
}

impl FileName {
    /// Truncate bytes followed by the first DIRSIZ bytes.
    ///
    /// # Safety
    ///
    /// `bytes` must not contain any NUL characters.
    pub unsafe fn from_bytes(bytes: &[u8]) -> &Self {
        debug_assert!(!bytes.contains(&0));
        // SAFETY: `&FileName` is layout-compatible with `[u8]` because of its
        // attribute `#[repr(transparent)]`. Also, the slice satisfies the
        // invariant of FileName because of the safety condition of this method
        // and the fact that its length is at most DIRSIZ.
        unsafe { &*(&bytes[..cmp::min(DIRSIZ, bytes.len())] as *const [u8] as *const Self) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }
}

#[repr(transparent)]
pub struct Path {
    // Invariant: the slice contains no NUL characters.
    inner: [u8],
}

impl Path {
    pub fn new(cstr: &CStr) -> &Self {
        // SAFETY: `&Path` is layout-compatible with `[u8]` because of its
        // attribute `#[repr(transparent)]`. Also, the slice does not contain
        // NUL according to the specification CStr::of to_bytes.
        unsafe { &*(cstr.to_bytes() as *const [u8] as *const Self) }
    }

    /// # Safety
    ///
    /// `bytes` must not contain any NUL bytes.
    pub unsafe fn from_bytes(bytes: &[u8]) -> &Self {
        // SAFETY: `&Path` is layout-compatible with `[u8]` because of its
        // attribute `#[repr(transparent)]`. Also, the slice does not contain
        // NUL according to the safety condition of this method.
        unsafe { &*(bytes as *const [u8] as *const Self) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    /// Returns `Some((path, name))` where,
    ///  - `name` is the next path element from `self`, and
    ///  - `path` is the remaining path.
    ///
    /// The returned path has no leading slashes, so the caller can check path.inner.is_empty() to
    /// see if the name is the last one.
    ///
    /// If no name to remove, returns `None`.
    ///
    /// # Examples
    /// ```
    /// # unsafe {
    /// assert_eq!(
    ///     Path::from_bytes(b"a/bb/c").skipelem(),
    ///     Some((Path::from_bytes(b"bb/c"), b"a")),
    /// );
    /// assert_eq!(
    ///     Path::from_bytes(b"///a//bb").skipelem(),
    ///     Some((Path::from_bytes(b"bb"), b"a")),
    /// );
    /// assert_eq!(
    ///     Path::from_bytes(b"a").skipelem(),
    ///     Some((Path::from_bytes(b""), b"a")),
    /// );
    /// assert_eq!(Path::from_bytes(b"").skipelem(), None);
    /// assert_eq!(Path::from_bytes(b"////").skipelem(), None);
    /// # }
    /// ```
    // TODO(https://github.com/kaist-cp/rv6/issues/359): Fix doctests work.
    pub fn skipelem(&self) -> Option<(&Self, &FileName)> {
        let mut bytes = &self.inner;

        let name_start = bytes.iter().position(|ch| *ch != b'/')?;
        bytes = &bytes[name_start..];

        let len = bytes
            .iter()
            .position(|ch| *ch == b'/')
            .unwrap_or(bytes.len());

        // SAFETY: `bytes` is a subslice of `self.inner`, which contains no NUL characters.
        let name = unsafe { FileName::from_bytes(&bytes[..len]) };

        bytes = &bytes[len..];

        let next_start = bytes
            .iter()
            .position(|ch| *ch != b'/')
            .unwrap_or(bytes.len());

        // SAFETY: `bytes` is a subslice of `self.inner`, which contains no NUL characters.
        let path = unsafe { Self::from_bytes(&bytes[next_start..]) };
        Some((path, name))
    }

    /// Returns `true` if `Path` begins with `'/'`.
    pub fn is_absolute(&self) -> bool {
        !self.inner.is_empty() && self.inner[0] == b'/'
    }

    pub fn is_empty_string(&self) -> bool {
        self.inner.is_empty()
    }
}
