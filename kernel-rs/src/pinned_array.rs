use core::pin::Pin;
use core::slice;

pub fn index_mut<T, const N: usize>(arr: Pin<&mut [T; N]>, index: usize) -> Pin<&mut T> {
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
            iter: unsafe { arr.get_unchecked_mut() }.into_iter()
        }
    }
}

impl<'s, T> Iterator for IterMut<'s, T> {
    type Item = Pin<&'s mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|p| unsafe { Pin::new_unchecked(p) })
    }
}
