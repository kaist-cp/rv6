//! A type that lets you distinguish instances of the same type at compile time.
//!
//! Often, one may want to distinguish multiple instances of the same type,
//! or express that an instance of type `U` was originated from a specific instance of type `T`,
//! but not another instance of the same type. `Branded` lets you do this at compile time.
//! In other words, `Branded` lets you use *branded types* (or *type generativity*).
//!
//! Concrete examples were we could use `Branded` are
//! * `Vec` and `VecIndex`,
//! * `Allocator` and `Box`,
//! * `Procs` and `Proc`,
//! * `Arena` and `ArenaRc`,
//! * `Lock` and `RemoteLock`,
//!
//! etc.
//!
//! For each case, you could
//! * make the `Branded` consume the type itself (such as `Branded<'id, Vec>`), or
//! * make it only hold a reference (such as `Branded<'id, &Vec>`) while storing the actual value at another place,
//!   so that you can use the actual value again after the `Branded` drops.
//!
//! # Example
//! The following is a simplified example.
//! Suppose that we have a `Library` type, that owns a lot of `Book`s.
//! Also, suppose that using `Library::borrow_book`, the caller can borrow a `Book` from the `Library`
//! in the form of a `BorrowedBook`, but needs to later return the `BorrowedBook` using `Library::return_book` to the same `Library`.
//! At first glance, one may think that the following code will suffice.
//! ```rust,no_run
//! pub struct Book {
//!     /* Omitted */
//! #   borrowed: Cell<bool>,
//! }
//! # impl Book {
//! #   fn new() -> Self {
//! #       Self {
//! #           borrowed: Cell::new(false),
//! #       }
//! #   }
//! # }
//!
//! pub struct BorrowedBook<'s> {
//!     /* Omitted */
//! #    index: usize,
//! #    _marker: PhantomData<&'s Book>,
//! }
//!
//! pub struct Library {
//!     books: [Book; 3],
//! }
//!
//! impl Library {
//!     pub fn new() -> Self {
//!         /* Omitted */
//! #        Self {
//! #            books: [Book::new(), Book::new(), Book::new()],
//! #        }
//!     }
//!
//!     pub fn borrow_book(&self) -> BorrowedBook<'_> {
//!         /* Omitted */
//! #       // Note: In the following, you can avoid using runtime check if you use more complex code.
//! #       for index in 0..self.books.len() {
//! #           if !self.books[index].borrowed.get() {
//! #               self.books[index].borrowed.set(true);
//! #               return BorrowedBook {
//! #                   index,
//! #                   _marker: PhantomData,
//! #               };
//! #           }
//! #       }
//! #       panic!("no unborrowed books left");
//!     }
//!
//!     pub fn return_book(&self, book: BorrowedBook<'_>) {
//!         /* Omitted */
//! #       // Note: In the following, you can avoid using runtime check if you use more complex code.
//! #       self.books[book.index].borrowed.set(false);
//!     }
//! }
//! ```
//! However, the following code causes a problem.
//! ```rust,no_run
//! let library_a = Library::new();
//! let library_b = Library::new();
//! let book_from_a = library_a.borrow_book();
//! library_b.return_book(book_from_a); // Returning a book from library_a to library_b!
//! ```
//! Note that in this case, the `book_from_a: BorrowedBook` was from `library_a`, but we are returning it to `library_b`.
//! To prevent this, we would need to use a runtime check in `Library::return_book` to check that the `book` was truely
//! from `self`, or we would need to mark `Library::return_book` as unsafe.
//!
//! Or, if we use branded types here, we don't need to use a runtime check or mark `Library::return_book` as unsafe,
//! but still express that the argument `book` must have originated from `self`. The following is an example,
//! where we just changed the signature of `Library::borrow_book` and `Library::return_book`.
//! ```rust,no_run
//! #![feature(arbitrary_self_types)] // just for convenience
//!
//! pub struct Book {
//!     /* Omitted */
//! #   borrowed: Cell<bool>,
//! }
//! # impl Book {
//! #   fn new() -> Self {
//! #       Self {
//! #           borrowed: Cell::new(false),
//! #       }
//! #   }
//! # }
//!
//! pub struct BorrowedBook<'id, 's> {
//!     /* Omitted */
//! #   index: Branded<'id, usize>,
//! #   _marker: PhantomData<&'s Book>,
//! }
//!
//! pub struct Library {
//!     books: [Book; 3],
//! }
//!
//! # #[derive(Clone, Copy)]
//! pub struct LibraryRef<'id, 's> {
//!    /* Omitted */
//! #  inner: Branded<'id, &'s Library>,
//! }
//! # impl<'id, 's> Deref for LibraryRef<'id, 's> {
//! #   type Target = Library;
//! #
//! #   fn deref(&self) -> &Self::Target {
//! #       &self.inner
//! #   }
//! # }
//!
//! impl Library {
//!     pub fn new() -> Self {
//!        /* Omitted */
//! #      Self {
//! #          books: [Book::new(), Book::new(), Book::new()],
//! #      }
//!     }
//!
//!     pub fn map_ref<F: for<'new_id> FnOnce(LibraryRef<'new_id, '_>) -> R, R>(&self, f: F) -> R {
//!        /* Omitted */
//! #      Branded::new(self, |inner| f(LibraryRef { inner }))
//!     }
//!
//!     pub fn borrow_book<'id, 's>(self: LibraryRef<'id, 's>) -> BorrowedBook<'id, 's> {
//!        /* Omitted */
//! #      // Note: In the following, you can avoid using runtime check if you use more complex code.
//! #      for index in 0..self.books.len() {
//! #          if !self.books[index].borrowed.get() {
//! #              self.books[index].borrowed.set(true);
//! #              return BorrowedBook {
//! #                  index: self.inner.brand(index),
//! #                  _marker: PhantomData,
//! #              };
//! #          }
//! #      }
//! #      panic!("no unborrowed books left");
//!     }
//!
//!     pub fn return_book<'id>(self: LibraryRef<'id, '_>, book: BorrowedBook<'id, '_>) {
//!        /* Omitted */
//! #      // Note: In the following, you can avoid using runtime check if you use more complex code.
//! #      self.books[book.index.into_inner()].borrowed.set(false);
//!     }
//! }
//! ```
//! In this case, the following code causes a compile error.
//! ```rust,no_run
//! let library_a = Library::new();
//! let library_b = Library::new();
//! library_a.map_ref(|library_a_ref| {
//!     library_b.map_ref(|library_b_ref| {
//!         let book_from_a = library_a_ref.borrow_book();
//!         library_b_ref.return_book(book_from_a); // Compile error because the `'id` tag is different!
//!     });
//! });
//! ```
//! This code causes a compile error because `Branded::new` tags an invariant lifetime to the provided pointer.
//! This lifetime is more like a unique identifier that cannot be subtyped by any other lifetime, not even `'static`.
//! This means that the lifetime `'id` attached to `library_a_ref` are `library_b_ref` are incompatible,
//! Also, note that
//! * `Library::borrow_book` returns a `BorrowedBook` that has the same `'id` tag with `self`, and
//! * `Library::return_book` only accepts `BorrowedBook`s that has the same `'id` tag with `self`.
//!
//! Therefore, if we try to do `library_b_ref.return_book(book_from_a);`, a compile error happens because
//! the lifetime `'id` attached to `library_b_ref` and `book_from_a` are incompatible.
//! Note that a compile error does not happen if we do `library_a_ref.return_book(book_from_a);` instead.
//!
//! # Adding invariants to branded types by using wrappers
//!
//! Note that `Branded::brand` is a safe function.
//! That is, for any data, you can safely obtain a `Branded` that adds the `'id` tag to it.
//! This is because a `Branded<'id, T>` is just a product of the `'id` tag and the data `T`.
//! **It does not have any invariants to it**.
//!
//! However, you may want to add invariants to branded types. For example, in the previous `Library` example,
//! we need to make sure
//! * If two or more `LibraryRef`s have the same `'id` tag, **they always point to the same `Library` instance**.
//! * A `BorrowedBook<'id, '_>` can only be obtained from a `LibraryRef<'id, '_>` that has the **same `'id` tag**.
//!
//! To do this, you should always wrap the `Branded` with your own wrapper (e.g. `LibraryRef` or `BorrowedBook`).
//! Then, you should make sure that wrapper can be obtained only through a controlled way.
//! That is,
//! * One can obtain a `Branded<'id, T>` for any data, but
//! * One can obtain the wrapper only through a restricted way.
//!
//! That is, you should wrap the `Branded` with your own wrapper,
//! and add invariants to the wrapper instead of the `Branded` itself.

use core::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

/// An invariant lifetime.
type Id<'id> = PhantomData<Cell<&'id mut ()>>;

/// A wrapper that adds an `'id` lifetime, which is used as a brand identifier.
/// * `Branded::new` returns a `Branded` that has an invariant, unique `'id`.
/// * `Branded::brand` returns a `Branded` that has the same `'id` with the provided `Branded`.
///   This is the only way to make a new `Branded` that has the same `'id` with another `Branded`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Branded<'id, T> {
    _id: Id<'id>,
    inner: T,
}

impl<'id, T> Branded<'id, T> {
    /// Creates a new `Branded` that has an invariant, unique `'id` attached.
    /// The new `Branded` can only be used within the given closure `f`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F: for<'new_id> FnOnce(Branded<'new_id, T>) -> R, R>(inner: T, f: F) -> R {
        f(Branded {
            _id: PhantomData,
            inner,
        })
    }

    /// Returns a new `Branded` that wraps `inner` and has the same `'id` with `self`.
    /// This is the only way to create a new `Branded` that has the same `'id` with another `Branded`.
    ///
    /// # Note
    ///
    /// This function is not an unsafe function. If you want the user to be able to obtain a `Branded`
    /// only through a restricted way, you should wrap the `Branded` instead.
    /// See the module documentation for details.
    pub fn brand<U>(&self, inner: U) -> Branded<'id, U> {
        Branded {
            _id: PhantomData,
            inner,
        }
    }

    /// Unwraps the `Branded<'id, T>` into `T`.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<'id, T> Deref for Branded<'id, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'id, T> DerefMut for Branded<'id, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
