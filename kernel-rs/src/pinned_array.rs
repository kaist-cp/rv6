use core::pin::Pin;
use core::slice;

/// Trys to return a pinned mutable reference of the array's element at index `index`.
/// Returns `Some(pin_mut)` if index is not out of bounds.
/// Otherwise, returns `None`.
pub fn get_pin_mut<T, const N: usize>(arr: Pin<&mut [T; N]>, index: usize) -> Option<Pin<&mut T>> {
    if index < N {
        // Safe since we're just projecting from a pinned array to a pinned mutable reference of its elements.
        Some(unsafe { Pin::new_unchecked(arr.get_unchecked_mut().get_unchecked_mut(index)) })
    } else {
        None
    }
}

/// An iterator that gives pinned mutable references to elements of a pinned array.
#[derive(Debug)]
pub struct IterPinMut<'s, T> {
    iter: slice::IterMut<'s, T>,
}

impl<'s, T, const N: usize> From<Pin<&'s mut [T; N]>> for IterPinMut<'s, T> {
    fn from(arr: Pin<&'s mut [T; N]>) -> Self {
        Self {
            // Safe since we only provide pinned mutable references to the outside.
            iter: unsafe { arr.get_unchecked_mut() }.iter_mut(),
        }
    }
}

impl<'s, T> Iterator for IterPinMut<'s, T> {
    type Item = Pin<&'s mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        // Safe since this iterator just projects from a pinned array to a pinned mutable reference of its elements.
        self.iter.next().map(|p| unsafe { Pin::new_unchecked(p) })
    }
}
