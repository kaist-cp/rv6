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
        let mut name: [u8; DIRSIZ] = [0; DIRSIZ];
        self.namex(0, &mut name)
    }

    pub unsafe fn nameiparent(&self, name: &mut [u8; DIRSIZ]) -> Result<*mut Inode, ()> {
        self.namex(1, name)
    }

    /// Paths
    ///
    /// Copy the next path element from path into name.
    /// Return a pointer to the element following the copied one.
    /// The returned path has no leading slashes,
    /// so the caller can check path.inner.is_empty() to see if the name is the last one.
    /// If no name to remove, return 0.
    ///
    /// Examples:
    ///   skipelem("a/bb/c", name) = "bb/c", setting name = "a"
    ///   skipelem("///a//bb", name) = "bb", setting name = "a"
    ///   skipelem("a", name) = "", setting name = "a"
    ///   skipelem("", name) = skipelem("////", name) = 0
    fn skipelem(&self, name: &mut [u8; DIRSIZ]) -> Option<&Self> {
        let mut bytes = &self.inner;

        let name_start = bytes.iter().position(|ch| *ch != b'/')?;
        bytes = &bytes[name_start..];

        let len = bytes
            .iter()
            .position(|ch| *ch == b'/')
            .unwrap_or(bytes.len());

        if len >= DIRSIZ as _ {
            name.copy_from_slice(&bytes[..DIRSIZ]);
        } else {
            name[..len].copy_from_slice(&bytes[..len]);
            name[len] = 0;
        }

        bytes = &bytes[len..];

        let next_start = bytes
            .iter()
            .position(|ch| *ch != b'/')
            .unwrap_or(bytes.len());

        // SAFETY: `bytes` is a subslice of `self.inner`, which contains no NUL characters.
        Some(unsafe { Self::from_bytes(&bytes[next_start..]) })
    }

    /// Returns `true` if `Path` begins with `'/'`.
    fn is_absolute(&self) -> bool {
        self.inner.len() != 0 && self.inner[0] == b'/'
    }

    /// Look up and return the inode for a path name.
    /// If parent != 0, return the inode for the parent and copy the final
    /// path element into name, which must have room for DIRSIZ bytes.
    /// Must be called inside a transaction since it calls Inode::put().
    unsafe fn namex(&self, nameiparent: i32, name: &mut [u8; DIRSIZ]) -> Result<*mut Inode, ()> {
        let mut ip = if self.is_absolute() {
            iget(ROOTDEV as u32, ROOTINO)
        } else {
            (*(*myproc()).cwd).idup()
        };

        let mut path = self;

        loop {
            path = some_or!(path.skipelem(name), break);

            (*ip).lock();
            if (*ip).typ as i32 != T_DIR {
                (*ip).unlockput();
                return Err(());
            }
            if nameiparent != 0 && path.inner.is_empty() {
                // Stop one level early.
                (*ip).unlock();
                return Ok(ip);
            }
            let next: *mut Inode = dirlookup(ip, name.as_mut_ptr(), ptr::null_mut());
            if next.is_null() {
                (*ip).unlockput();
                return Err(());
            }
            (*ip).unlockput();
            ip = next
        }
        if nameiparent != 0 {
            (*ip).put();
            return Err(());
        }
        Ok(ip)
    }
}
