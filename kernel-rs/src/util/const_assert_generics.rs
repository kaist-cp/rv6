//! Types that let you compile-time assert in `where` clauses,
//! especially about the input generic parameters.
//!
//! # Example
//! ```rust,no_run
//! # use core::mem;
//! #![feature(const_generics)]
//! #![feature(const_evaluatable_checked)]
//!
//! unsafe fn transmute<T, U>(t: T) -> U
//! where
//!     Assert2<
//!         { mem::size_of::<T>() == mem::size_of::<U>() },
//!         { mem::align_of::<T>() == mem::align_of::<U>() },
//!     >: True
//! {
//!     /* Omitted */
//! }
//! ```

pub struct Assert<const EXPR: bool>;
pub struct Assert2<const EXPR: bool, const EXPR2: bool>;
pub struct Assert3<const EXPR: bool, const EXPR2: bool, const EXPR3: bool>;

pub trait True {}
impl True for Assert<true> {}
impl True for Assert2<true, true> {}
impl True for Assert3<true, true, true> {}
