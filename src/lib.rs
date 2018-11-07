//! A 'snitching' atomically reference counted pointer.
//!
//! `Narc`s are almost transparent wrappers around atomic reference counts (`std::sync::Arc`)
//! that track where references originated from. By recording the initial creation, as well as any
//! operation that results in a new reference, such as a cloning or downgrading, any reference is
//! always able to tell what its siblings pointing to the same value are.
//!
//! This functionality is useful for manually tracking down reference cycles or other causes that
//! prevent proper clean-up, which occasionally result in deadlocks.
//!
//! `Narc` and `Weak` are drop-in replacements for `Arc` and `Weak` respectively. However, calling
//! the standard interfaces does not allow complete call site information to be added, so methods
//! like `new_at_line` or `clone_at_line` should be used. In case the compatible methods like `new`,
//! `clone`, ... are called, `Site::Unknown` is used for the resulting tracked `Origin`.
//!
//! TODO: Example on how to use, including `file!` and `line!` macros.

pub mod tracing;

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex, Weak as ArcWeak};
use tracing::{Origin, OriginKind, Site, Uid};

/// A 'snitching' atomically reference counted pointer.
///
/// A `Narc` wraps an actual `Arc` and assigns it a unique ID upon creation. Any offspring of
/// created via `clone` or `downgrade` is tracked by being assigned a unique ID as well. If the
/// annotating methods `new_at_line`, `clone_at_line`, etc. are used, the `Narc` will also know
/// its origin.
#[derive(Debug)]
pub struct Narc<T> {
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

/// Inner state of `Narc`.
#[derive(Debug)]
struct Inner<T> {
    /// The actual value.
    data: T,
    /// Sibling metadata.
    map: Mutex<Map>,
}

/// The non-owned version of a `Narc`.
#[derive(Debug)]
pub struct Weak<T> {
    /// Wrapped non-owned [std::sync] arc reference.
    inner: ArcWeak<Inner<T>>,
    /// Unique ID for this instance.
    id: Uid,
}

impl<T> Narc<T> {
    /// Internal instantiation function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with `OriginKind::New`.
    fn new_at_site(data: T, site: Site) -> Narc<T> {
        let origin = Origin {
            kind: OriginKind::New,
            site,
        };

        let mut map = Map::new();
        let id = map.next_id();
        map.strongs.insert(id, origin);

        Narc {
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
    fn clone_at_site(&self, site: Site) -> Narc<T> {
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

        Narc {
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
            id: new_id,
        }
    }

    /// Returns a new `Narc` with the provided file name and line as the origin.
    pub fn new_at_line(data: T, file: &'static str, line: u32) -> Narc<T> {
        Narc::new_at_site(data, Site::SourceFile { file, line })
    }

    /// Creates a new `Weak` pointer to this value.
    pub fn clone_at_line(&self, file: &'static str, line: u32) -> Narc<T> {
        self.clone_at_site(Site::SourceFile { file, line })
    }

    /// Creates a new `Weak` pointer to this value.
    pub fn downgrade_at_line(this: &Self, file: &'static str, line: u32) -> Weak<T> {
        Narc::downgrade_at_site(this, Site::SourceFile { file, line })
    }

    /// Returns the contained value if the `Narc` has exactly one strong reference.
    pub fn try_unwrap(_this: Self) -> Result<T, Self> {
        // TODO: Come up with a clever way to impl this.
        // Arc::try_unwrap(this.inner)
        //     .map(|i| i.data)
        //     .map_err(|i| Narc { inner: i })
        unimplemented!()
    }

    /// Gets the number of `Weak` pointers to this value.
    ///
    /// See `std::sync::Arc::weak_count` for details.
    pub fn weak_count(_this: &Narc<T>) -> usize {
        unimplemented!()
    }

    /// Gets the number of `Narc` pointers to this value.
    ///
    /// See `std::sync::Arc::strong_count` for details.
    pub fn strong_count(_this: &Narc<T>) -> usize {
        unimplemented!()
    }

    /// Returns true if the two Arcs point to the same value (not just values that compare as equal).
    ///
    /// See `std::sync::Arc::ptr_eq` for details.
    pub fn ptr_eq(_this: &Narc<T>, _other: &Narc<T>) -> bool {
        unimplemented!()
    }

    /// Makes a mutable reference into the given Arc.
    ///
    /// See `std::sync::Arc::make_mut` for details.
    pub fn make_mut(_this: &mut Narc<T>) -> &mut T {
        unimplemented!()
    }

    /// Returns a mutable reference to the inner value, if there are no other Arc or Weak pointers
    /// to the same value.
    ///
    /// See `std::sync::Arc::make_mut` for details.
    pub fn get_mut(_this: &mut Narc<T>) -> Option<&mut T> {
        unimplemented!()
    }
}

impl<T: Deref> Deref for Narc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.data
    }
}

impl<T> Drop for Narc<T> {
    fn drop(&mut self) {
        let mut map = self.inner.map.lock().unwrap();
        map.strongs
            .remove(&self.id)
            .expect("Internal consistency error (drop)");
    }
}

// TODO: Clone impl.

impl<T> Weak<T> {
    /// Internal upgrade function.
    ///
    /// Directly accepts a `Site` instance, creates the correct `Origin` with
    /// `OriginKind::UpgradedFrom`.
    pub fn upgrade_at_site(&self, site: Site) -> Option<Narc<T>> {
        self.inner.upgrade().map(|inner| {
            let id = {
                let mut map = inner.map.lock().unwrap();
                let prev_origin = map
                    .weaks
                    .get(&self.id)
                    .expect("Internal consistency error (upgrade)")
                    .clone();
                let new_origin = Origin {
                    kind: OriginKind::UpgradedFrom(self.id, Box::new(prev_origin)),
                    site,
                };
                let new_id = map.next_id();
                map.strongs.insert(new_id, new_origin);
                new_id
            };
            Narc { inner, id }
        })
    }

    /// Attempts to upgrade the Weak pointer to an Arc, extending the lifetime of the value if
    /// successful.
    ///
    /// See `std::sync::Weak::upgrade` for details.
    pub fn upgrade_at_line(&self, file: &'static str, line: u32) -> Option<Narc<T>> {
        self.upgrade_at_site(Site::SourceFile { file, line })
    }
}

impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let mut map = inner.map.lock().unwrap();
            map.weaks
                .remove(&self.id)
                .expect("Internal consistency error (drop)");
        }
    }
}

// TODO: Implement Clone for Weak.

#[cfg(test)]
mod tests {
    use super::Narc;

    #[test]
    fn basic() {
        let thing = ();
        let thing_strong_0 = Narc::new_at_line(thing, file!(), line!());
        let thing_strong_1 = thing_strong_0.clone_at_line(file!(), line!());
        let thing_weak_0 = Narc::downgrade_at_line(&thing_strong_0, file!(), line!());
        let thing_weak_1 = Narc::downgrade_at_line(&thing_strong_0, file!(), line!());
        let thing_strong_2 = thing_weak_0.upgrade_at_line(file!(), line!());

        println!("\nthing_strong_0: {:?}", thing_strong_0);
        println!("\nthing_strong_1: {:?}", thing_strong_1);
        println!("\nthing_weak_0: {:?}", thing_weak_0);
        println!("\nthing_weak_1: {:?}", thing_weak_1);
        println!("\nthing_strong_2: {:?}", thing_strong_2);

        // TODO: Actually check something.
    }
}
