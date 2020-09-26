use core::cmp;
use core::ptr;
use cstr_core::CStr;

use super::{dirlookup, iget, Inode, DIRSIZ, ROOTDEV, ROOTINO, T_DIR};
use crate::proc::myproc;
use crate::some_or;

#[repr(transparent)]
pub struct Path {
    // Invariant: the slice contains no NUL characters.
    inner: [u8],
}

impl Path {
    pub fn new(cstr: &CStr) -> &Self {
        // SAFETY: `&Path` is layout-compatible with `[u8]` because of  its attribute
        // `#[repr(transparent)]`.
        unsafe { &*(cstr.to_bytes() as *const [u8] as *const Self) }
    }

    /// # Safety
    ///
    /// `bytes` must not contain any NUL bytes.
    pub unsafe fn from_bytes(bytes: &[u8]) -> &Self {
        &*(bytes as *const [u8] as *const Self)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    pub unsafe fn namei(&self) -> Result<*mut Inode, ()> {
        Ok(self.namex(false)?.0)
    }

    pub unsafe fn nameiparent(&self) -> Result<(*mut Inode, &[u8]), ()> {
        let (ip, name_in_path) = self.namex(true)?;
        let name_in_path = name_in_path.ok_or(())?;
        Ok((ip, name_in_path))
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
    // TODO: Make an iterator.
    // TODO: Fix doctests work.
    fn skipelem(&self) -> Option<(&Self, &[u8])> {
        let mut bytes = &self.inner;

        let name_start = bytes.iter().position(|ch| *ch != b'/')?;
        bytes = &bytes[name_start..];

        let len = bytes
            .iter()
            .position(|ch| *ch == b'/')
            .unwrap_or(bytes.len());

        // Truncate bytes followed by the first DIRSIZ bytes.
        let name = &bytes[..cmp::min(len, DIRSIZ)];

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
    fn is_absolute(&self) -> bool {
        self.inner.len() != 0 && self.inner[0] == b'/'
    }

    /// Look up and return the inode for a path name.
    /// If parent != 0, return the inode for the parent and copy the final
    /// path element into name, which must have room for DIRSIZ bytes.
    /// Must be called inside a transaction since it calls Inode::put().
    unsafe fn namex(&self, parent: bool) -> Result<(*mut Inode, Option<&[u8]>), ()> {
        let mut ip = if self.is_absolute() {
            iget(ROOTDEV as u32, ROOTINO)
        } else {
            (*(*myproc()).cwd).idup()
        };

        let mut path = self;

        loop {
            let (new_path, name) = some_or!(path.skipelem(), break);
            path = new_path;

            (*ip).lock();
            if (*ip).typ as i32 != T_DIR {
                (*ip).unlockput();
                return Err(());
            }
            if parent && path.inner.is_empty() {
                // Stop one level early.
                (*ip).unlock();
                return Ok((ip, Some(name)));
            }
            let next: *mut Inode = dirlookup(ip, name, ptr::null_mut());
            if next.is_null() {
                (*ip).unlockput();
                return Err(());
            }
            (*ip).unlockput();
            ip = next
        }
        if parent {
            (*ip).put();
            return Err(());
        }
        Ok((ip, None))
    }
}
