//! A 'snitching' atomically reference counted pointer.
//!
//! `Snarc`s are almost transparent wrappers around atomic reference counts (`std::sync::Arc`)
//! that track where references originated from. By recording the initial creation, as well as any
//! operation that results in a new reference, such as a cloning or downgrading, any reference is
//! always able to tell what its siblings pointing to the same value are.
//!
//! This functionality is useful for manually tracking down reference cycles or other causes that
//! prevent proper clean-up, which occasionally result in deadlocks.
//!
//! `Snarc` and `Weak` are drop-in replacements for `Arc` and `Weak` respectively. However, calling
//! the standard interfaces does not allow complete call site information to be added, so methods
//! like `new_at_line` or `clone_at_line` should be used. In case the compatible methods like `new`,
//! `clone`, ... are called, `Site::Unknown` is used for the resulting tracked `Origin`.
//!
//! ```rust
//! use snarc::Snarc;
//!
//! // Snarc keeps track of every instatiation, clone, upgrade and downgrade:
//! let foo = Snarc::new_at_line(vec![1.0, 2.0, 3.0], file!(), line!());
//!
//! // A file/line annotated clone of the reference. The new reference will have a record
//! // of its origin and the line where the cloning happened.
//! let a = foo.clone_at_line(file!(), line!());
//!
//! // "Regular" clone. This will not record file and line information.
//! let b = Snarc::clone(&foo);
//! ```
//!
//! In most cases, `Snarc` can be used as a quick drop-in replacement:
//!
//! ```rust
//! use snarc::Snarc as Arc;
//!
//! let bar = Arc::new(vec![1.0, 2.0, 3.0]);
//! ```
//!
//! This form allows only some instances to be annotated, or annotations being added gradually.

#![feature(coerce_unsized)]
#![feature(unsize)]

pub mod tracing;

use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, CoerceUnsized};
use std::sync::{Arc, Mutex, Weak as ArcWeak};
use std::marker::Unsize;
use std::borrow;

use tracing::{Origin, OriginKind, Site, Uid};

/// Tracked reference state.
///
/// The `Map` tracks the number and site of references pointing toward the same value.
#[derive(Debug)]
struct Map {
    strongs: HashMap<Uid, Origin>,
    weaks: HashMap<Uid, Origin>,
    next_id: Uid,
}

impl Map {
    /// Creates a new map instance.
    fn new() -> Map {
        Map {
            strongs: HashMap::with_capacity(128),
            weaks: HashMap::with_capacity(128),
            next_id: 0,
        }
    }

    /// Increments the `next_id` counter and returns the previous value.
    fn next_id(&mut self) -> Uid {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Inner state of `Snarc`.
#[derive(Debug)]
struct Inner<T: ?Sized> {
    /// Sibling metadata.
    map: Mutex<Map>,
    /// The actual value.
    data: T,
}

/// A 'snitching' atomically reference counted pointer.
///
/// A `Snarc` wraps an actual `Arc` and assigns it a unique ID upon creation. Any offspring of
/// created via `clone` or `downgrade` is tracked by being assigned a unique ID as well. If the
/// annotating methods `new_at_line`, `clone_at_line`, etc. are used, the `Snarc` will also know
/// its origin.
#[derive(Debug)]
pub struct Snarc<T: ?Sized> {
    /// Wrapped [std::sync] arc reference.
    inner: Arc<Inner<T>>,
    /// Unique ID for this instance.
    id: Uid,
}

impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<Snarc<U>> for Snarc<T> {}

/// The non-owned version of a `Snarc`.
#[derive(Debug)]
pub struct Weak<T: ?Sized> {
    /// Unique ID for this instance.
    id: Option<Uid>,
    /// Wrapped non-owned [std::sync] arc reference.
    inner: ArcWeak<Inner<T>>,
}

impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<Weak<U>> for Weak<T> {}

impl<T> Snarc<T> {
    /// Internal instantiation function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with `OriginKind::New`.
    fn new_at_site(data: T, site: Site) -> Snarc<T> {
        let mut map = Map::new();
        let id = map.next_id();

        let origin = Origin {
            kind: OriginKind::New,
            site,
            id,
        };

        map.strongs.insert(id, origin);

        Snarc {
            inner: Arc::new(Inner {
                data,
                map: Mutex::new(map),
            }),
            id,
        }
    }

    /// Returns a new `Snarc` with the provided file name and line as the origin.
    pub fn new_at_line(data: T, file: &'static str, line: u32) -> Snarc<T> {
        Snarc::new_at_site(data, Site::SourceFile { file, line })
    }

    /// Creates new `Snarc` with unknown origin.
    ///
    /// If possible, use `new_at_line` instead.
    pub fn new(data: T) -> Snarc<T> {
        Snarc::new_at_site(data, Site::Unknown)
    }

    /// Returns the contained value if the `Snarc` has exactly one strong reference.
    pub fn try_unwrap(_this: Self) -> Result<T, Self> {
        // TODO: Make this work (currently, drop is an issue).

        // let Snarc { inner, id } = this;

        // match Arc::try_unwrap(inner) {
        //     Ok(inner) => {
        //         // We've dissolved our Snarc, as we are the last strong reference. All that's left
        //         // are weak references, so our copy of `map` is the last one surviving and will
        //         // be freed once we exit this function. We do not need to clean up for this reason.
        //         Ok(inner.data)
        //     }
        //     Err(new_inner) => Err(Snarc {
        //         inner: new_inner,
        //         id,
        //     }),
        // }
        unimplemented!()
    }
}

impl<T: ?Sized> Snarc<T> {
    /// Internal cloning function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::Cloned`.
    fn clone_at_site(&self, site: Site) -> Snarc<T> {
        let mut map = self.inner.map.lock().unwrap();
        let parent_origin = map
            .strongs
            .get(&self.id)
            .expect("Internal consistency error (clone). This should never happen.")
            .clone();
        let new_id = map.next_id();
        let new_origin = Origin {
            kind: OriginKind::Cloned(Box::new(parent_origin)),
            site,
            id: new_id,
        };
        map.strongs.insert(new_id, new_origin);

        Snarc {
            inner: self.inner.clone(),
            id: new_id,
        }
    }

    /// Internal downgrade function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::Downgraded`.
    fn downgrade_at_site(this: &Self, site: Site) -> Weak<T> {
        let mut map = this.inner.map.lock().unwrap();
        // No need to `::remove` here because the strong ref will be dropped.
        let prev_origin = map
            .strongs
            .get(&this.id)
            .expect("Internal consistency error (downgrade). This should never happen.")
            .clone();
        let new_id = map.next_id();
        let new_origin = Origin {
            kind: OriginKind::Downgraded(Box::new(prev_origin)),
            site,
            id: new_id,
        };
        map.weaks.insert(new_id, new_origin);

        Weak {
            inner: Arc::downgrade(&this.inner),
            id: Some(new_id),
        }
    }

    /// Clones `Snarc` with the provided file name and line as the origin.
    pub fn clone_at_line(&self, file: &'static str, line: u32) -> Snarc<T> {
        self.clone_at_site(Site::SourceFile { file, line })
    }

    /// Creates a new `Weak` pointer to this value with the provided file name and line as the
    /// origin.
    pub fn downgrade_at_line(this: &Self, file: &'static str, line: u32) -> Weak<T> {
        Snarc::downgrade_at_site(this, Site::SourceFile { file, line })
    }

    /// Creates a new `Weak` pointer to this value.
    ///
    /// If possible, use `new_at_line` instead.
    pub fn downgrade(this: &Self) -> Weak<T> {
        Snarc::downgrade_at_site(this, Site::Unknown)
    }

    /// Gets the number of `Weak` pointers to this value.
    ///
    /// See `std::sync::Arc::weak_count` for details.
    pub fn weak_count(this: &Snarc<T>) -> usize {
        Arc::weak_count(&this.inner)
    }

    /// Gets the number of `Snarc` pointers to this value.
    ///
    /// See `std::sync::Arc::strong_count` for details.
    pub fn strong_count(this: &Snarc<T>) -> usize {
        Arc::strong_count(&this.inner)
    }

    /// Returns true if the two Arcs point to the same value (not just values that compare as equal).
    ///
    /// See `std::sync::Arc::ptr_eq` for details.
    pub fn ptr_eq(this: &Snarc<T>, other: &Snarc<T>) -> bool {
        Arc::ptr_eq(&this.inner, &other.inner)
    }

    /// Returns a mutable reference to the inner value, if there are no other Arc or Weak pointers
    /// to the same value.
    ///
    /// See `std::sync::Arc::make_mut` for details.
    pub fn get_mut(this: &mut Snarc<T>) -> Option<&mut T> {
        Arc::get_mut(&mut this.inner).map(|inner| &mut inner.data)
    }

    /// Returns the origin chain of this reference.
    ///
    /// The resulting `Origin` can be printed using `fmt::Display`, see the `tracing` docs for
    /// details.
    pub fn origin(this: &Snarc<T>) -> Origin {
        this.inner
            .map
            .lock()
            .expect("Poisoned strong mapping. This is a bug.")
            .strongs
            .get(&this.id)
            .expect("Internal consisency error (origin). This is a bug.")
            .clone()
    }

    /// Returns the origin of the reference and all of its siblings.
    ///
    /// Returns a tuple of (strong origins, weak origins), including all live references.
    pub fn family(this: &Snarc<T>) -> (Vec<Origin>, Vec<Origin>) {
        let map = this
            .inner
            .map
            .lock()
            .expect("Poisoned strong mapping. This is a bug.");

        (
            map.strongs.values().cloned().collect(),
            map.weaks.values().cloned().collect(),
        )
    }
}

impl<T: Clone> Snarc<T> {
    /// Makes a mutable reference into the given Arc.
    ///
    /// See `std::sync::Arc::make_mut` for details.
    pub fn make_mut(_this: &mut Snarc<T>) -> &mut T {
        unimplemented!()
    }
}

impl<T: ?Sized> Deref for Snarc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner.data
    }
}

impl<T: ?Sized> Drop for Snarc<T> {
    fn drop(&mut self) {
        let mut map = self.inner.map.lock().unwrap();
        map.strongs
            .remove(&self.id)
            .expect("Internal consistency error (drop)");
    }
}

impl<T: ?Sized> Clone for Snarc<T> {
    fn clone(&self) -> Self {
        self.clone_at_site(Site::Unknown)
    }
}

impl<T: ?Sized> borrow::Borrow<T> for Snarc<T> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized> AsRef<T> for Snarc<T> {
    fn as_ref(&self) -> &T {
        &**self
    }
}


impl<T: ?Sized> Weak<T> {
    /// Internal upgrade function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::Upgraded`.
    pub fn upgrade_at_site(&self, site: Site) -> Option<Snarc<T>> {
        let id = self.id?;

        self.inner.upgrade().map(|inner| {
            let id = {
                let mut map = inner.map.lock().unwrap();
                let prev_origin = map
                    .weaks
                    .get(&id)
                    .expect("Internal consistency error (upgrade)")
                    .clone();
                let new_id = map.next_id();
                let new_origin = Origin {
                    kind: OriginKind::Upgraded(Box::new(prev_origin)),
                    site,
                    id: new_id,
                };
                map.strongs.insert(new_id, new_origin);
                new_id
            };
            Snarc { inner, id }
        })
    }

    /// Internal cloning function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::Cloned`.
    fn clone_at_site(&self, site: Site) -> Weak<T> {
        // We need to create a temporary untracked strong reference here, no way around it.
        //
        // The issue is that we need access to the data, which might be gone already, real `Weak`s
        // never have this issue.

        match self.inner.upgrade() {
            Some(strong) => {
                // The accompanying strong reference still exists, so we can perform a "proper"
                // clone.
                let mut map = strong.map.lock().unwrap();

                let our_id = self.id.expect(
                    "Succesfully upgraded a weak reference, but it has no ID.\
                     This should never happen.",
                );

                let parent_origin = map
                    .weaks
                    .get(&our_id)
                    .expect("Internal consistency error (weak clone). This should never happen.")
                    .clone();
                let new_id = map.next_id();
                let new_origin = Origin {
                    kind: OriginKind::Cloned(Box::new(parent_origin)),
                    site,
                    id: new_id,
                };
                map.weaks.insert(new_id, new_origin);

                Weak {
                    inner: self.inner.clone(),
                    id: Some(new_id),
                }
            }
            None => {
                // We cloned a dead weak ref. We already lost all of our tracking info, so there
                // is nothing we can do. Just hand out a weak ref, with no ID.
                Weak {
                    inner: self.inner.clone(),
                    id: None,
                }
            }
        }
    }

    /// Attempts to upgrade the Weak pointer to an Arc, extending the lifetime of the value if
    /// successful.
    ///
    /// See `std::sync::Weak::upgrade` for details.
    pub fn upgrade_at_line(&self, file: &'static str, line: u32) -> Option<Snarc<T>> {
        self.upgrade_at_site(Site::SourceFile { file, line })
    }

    /// Attempts to upgrade the Weak pointer to an Arc, extending the lifetime of the value if
    /// successful.
    ///
    /// If possible, use `upgrade_at_line` instead.
    pub fn upgrade(&self) -> Option<Snarc<T>> {
        self.upgrade_at_site(Site::Unknown)
    }
}

impl<T: ?Sized> Drop for Weak<T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let mut map = inner.map.lock().unwrap();
            let our_id = self
                .id
                .expect("No ID on alive weak reference in drop. This is a bug.");

            map.weaks
                .remove(&our_id)
                .expect("Internal consistency error (drop). This is a bug.");
        }
    }
}

impl<T: ?Sized> Clone for Weak<T> {
    fn clone(&self) -> Self {
        self.clone_at_site(Site::Unknown)
    }
}

// TODO: impl
//
// impl<T> Default for Weak<T> {
//
// impl<T: ?Sized + PartialEq> PartialEq for Snarc<T> {
// impl<T: ?Sized + PartialOrd> PartialOrd for Snarc<T> {
// impl<T: ?Sized + Ord> Ord for Snarc<T> {
// impl<T: ?Sized + Eq> Eq for Snarc<T> {}
// impl<T: ?Sized + fmt::Display> fmt::Display for Snarc<T> {
// impl<T: ?Sized + fmt::Debug> fmt::Debug for Snarc<T> { // Manual impl?
// impl<T: ?Sized> fmt::Pointer for Snarc<T> {
// impl<T: Default> Default for Snarc<T> {
// impl<T: ?Sized + Hash> Hash for Snarc<T> {
// impl<T> From<T> for Snarc<T> {
// impl<'a, T: Clone> From<&'a [T]> for Snarc<[T]> {
// impl<'a> From<&'a str> for Snarc<str> {
// impl From<String> for Snarc<str> {
// impl<T: ?Sized> From<Box<T>> for Snarc<T> {
// impl<T> From<Vec<T>> for Snarc<[T]> {


/// Output helper.
///
/// The `Dump` struct can be used as a zero-sized wrapper to output a `Snarc`. Example:
///
/// ```rust
/// use snarc::{Dump, Snarc};
///
/// let foo = Snarc::new(123);
/// let bar = Snarc::clone_at_line(&foo, file!(), line!());
/// let weak = Snarc::downgrade(&bar);
///
/// println!("{}", Dump(&bar));
/// ```
///
/// The resulting output will be something resembling:
///
/// ```ignore
/// Family associated with ID: 1
/// S| new<0>[?]
/// S| clone<1>[src/lib.rs:475] <- new<0>[?]
/// W| downgrade<2>[?] <- clone<1>[src/lib.rs:475] <- new<0>[?]
/// ```
#[derive(Debug)]
pub struct Dump<'a, T: 'a>(pub &'a Snarc<T>);

impl<'a, T: 'a> fmt::Display for Dump<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Family associated with ID: {}", self.0.id)?;

        let (mut strongs, mut weaks) = Snarc::family(self.0);

        // Sort by ID.
        strongs.sort();
        weaks.sort();

        for strong in strongs {
            writeln!(f, "S| {}", strong)?;
        }
        for weak in weaks {
            writeln!(f, "W| {}", weak)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Snarc;

    #[test]
    fn basic() {
        let thing = ();
        let thing_strong_0 = Snarc::new_at_line(thing, file!(), line!());
        let thing_strong_1 = thing_strong_0.clone_at_line(file!(), line!());
        let thing_weak_0 = Snarc::downgrade_at_line(&thing_strong_0, file!(), line!());
        let thing_weak_1 = Snarc::downgrade_at_line(&thing_strong_0, file!(), line!());
        let thing_strong_2 = thing_weak_0.upgrade_at_line(file!(), line!());

        println!("\nthing_strong_0: {:?}", thing_strong_0);
        println!("\nthing_strong_1: {:?}", thing_strong_1);
        println!("\nthing_weak_0: {:?}", thing_weak_0);
        println!("\nthing_weak_1: {:?}", thing_weak_1);
        println!("\nthing_strong_2: {:?}", thing_strong_2);

        // TODO: Actually check something.
    }
}
