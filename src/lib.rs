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

pub mod tracing;

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex, Weak as ArcWeak};
use tracing::{Origin, OriginKind, Site, Uid};

/// A 'snitching' atomically reference counted pointer.
///
/// A `Snarc` wraps an actual `Arc` and assigns it a unique ID upon creation. Any offspring of
/// created via `clone` or `downgrade` is tracked by being assigned a unique ID as well. If the
/// annotating methods `new_at_line`, `clone_at_line`, etc. are used, the `Snarc` will also know
/// its origin.
#[derive(Debug)]
pub struct Snarc<T> {
    /// Wrapped [std::sync] arc reference.
    inner: Arc<Inner<T>>,
    /// Unique ID for this instance.
    id: Uid,
}

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
struct Inner<T> {
    /// The actual value.
    data: T,
    /// Sibling metadata.
    map: Mutex<Map>,
}

/// The non-owned version of a `Snarc`.
#[derive(Debug)]
pub struct Weak<T> {
    /// Wrapped non-owned [std::sync] arc reference.
    inner: ArcWeak<Inner<T>>,
    /// Unique ID for this instance.
    id: Option<Uid>,
}

impl<T> Snarc<T> {
    /// Internal instantiation function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with `OriginKind::New`.
    fn new_at_site(data: T, site: Site) -> Snarc<T> {
        let origin = Origin {
            kind: OriginKind::New,
            site,
        };

        let mut map = Map::new();
        let id = map.next_id();
        map.strongs.insert(id, origin);

        Snarc {
            inner: Arc::new(Inner {
                data,
                map: Mutex::new(map),
            }),
            id,
        }
    }

    /// Internal cloning function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::ClonedFrom`.
    fn clone_at_site(&self, site: Site) -> Snarc<T> {
        let mut map = self.inner.map.lock().unwrap();
        let parent_origin = map
            .strongs
            .get(&self.id)
            .expect("Internal consistency error (clone). This should never happen.")
            .clone();
        let new_origin = Origin {
            kind: OriginKind::ClonedFrom(self.id, Box::new(parent_origin)),
            site,
        };
        let new_id = map.next_id();
        map.strongs.insert(new_id, new_origin);

        Snarc {
            inner: self.inner.clone(),
            id: new_id,
        }
    }

    /// Internal downgrade function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::DowngradedFrom`.
    fn downgrade_at_site(this: &Self, site: Site) -> Weak<T> {
        let mut map = this.inner.map.lock().unwrap();
        // No need to `::remove` here because the strong ref will be dropped.
        let prev_origin = map
            .strongs
            .get(&this.id)
            .expect("Internal consistency error (downgrade). This should never happen.")
            .clone();
        let new_origin = Origin {
            kind: OriginKind::DowngradedFrom(this.id, Box::new(prev_origin)),
            site,
        };
        let new_id = map.next_id();
        map.weaks.insert(new_id, new_origin);

        Weak {
            inner: Arc::downgrade(&this.inner),
            id: Some(new_id),
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
}

impl<T> Snarc<T>
where
    T: Clone,
{
    /// Makes a mutable reference into the given Arc.
    ///
    /// See `std::sync::Arc::make_mut` for details.
    pub fn make_mut(_this: &mut Snarc<T>) -> &mut T {
        unimplemented!()
    }
}

impl<T: Deref> Deref for Snarc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.data
    }
}

impl<T> Drop for Snarc<T> {
    fn drop(&mut self) {
        let mut map = self.inner.map.lock().unwrap();
        map.strongs
            .remove(&self.id)
            .expect("Internal consistency error (drop)");
    }
}

impl<T> Clone for Snarc<T> {
    fn clone(&self) -> Self {
        self.clone_at_site(Site::Unknown)
    }
}

impl<T> Weak<T> {
    /// Internal upgrade function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::UpgradedFrom`.
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
                let new_origin = Origin {
                    kind: OriginKind::UpgradedFrom(id, Box::new(prev_origin)),
                    site,
                };
                let new_id = map.next_id();
                map.strongs.insert(new_id, new_origin);
                new_id
            };
            Snarc { inner, id }
        })
    }

    /// Internal cloning function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::ClonedFrom`.
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
                let new_origin = Origin {
                    kind: OriginKind::ClonedFrom(our_id, Box::new(parent_origin)),
                    site,
                };
                let new_id = map.next_id();
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

impl<T> Drop for Weak<T> {
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

impl<T> Clone for Weak<T> {
    fn clone(&self) -> Self {
        self.clone_at_site(Site::Unknown)
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
