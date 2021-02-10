use core::pin::Pin;
use core::slice;

pub fn index_mut<T, const N: usize>(arr: Pin<&mut [T; N]>, index: usize) -> Pin<&mut T> {
    // Safe since we're just projecting from a pinned array to a pinned mutable reference of its elements.
    unsafe { Pin::new_unchecked(&mut arr.get_unchecked_mut()[index]) }
}

/// An iterator that gives pinned mutable references to elements of a pinned array.
#[derive(Debug)]
pub struct IterMut<'s, T> {
    iter: slice::IterMut<'s, T>,
}

impl<'s, T, const N: usize> From<Pin<&'s mut [T; N]>> for IterMut<'s, T> {
    fn from(arr: Pin<&'s mut [T; N]>) -> Self {
        Self {
            // Safe since we only provide pinned mutable references to the outside.
            iter: unsafe { arr.get_unchecked_mut() }.iter_mut(),
        }
    }
}

impl<'s, T> Iterator for IterMut<'s, T> {
    type Item = Pin<&'s mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        // Safe since this iterator just projects from a pinned array to a pinned mutable reference of its elements.
        self.iter.next().map(|p| unsafe { Pin::new_unchecked(p) })
    }
}
