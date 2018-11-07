//! A 'snitching' atomically reference counted pointer.
//!
//! [`Narc`]s are almost transparent wrappers around atomic reference counts ([`std::sync::Arc`])
//! that track where references originated from. By recording the initial creation, as well as any
//! operation that results in a new reference, such as a cloning or downgrading, any reference is
//! always able to tell what its siblings pointing to the same value are.
//!
//! This functionality is useful for manually tracking down reference cycles or other causes that
//! prevent proper clean-up, which occasionally result in deadlocks.
//!
//! TODO: Example on how to use.

#![allow(dead_code, unused_imports, unused_variables)]

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
    inner: Arc<Inner<T>>,
    id: Uid,
}

#[derive(Debug)]
struct Map {
    strongs: HashMap<Uid, Origin>,
    weaks: HashMap<Uid, Origin>,
    next_id: Uid,
}

impl Map {
    fn new() -> Map {
        Map {
            strongs: HashMap::with_capacity(128),
            weaks: HashMap::with_capacity(128),
            next_id: 0,
        }
    }

    // Increments the `next_id` counter and returns the previous value.
    fn next_id(&mut self) -> Uid {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[derive(Debug)]
struct Inner<T> {
    data: T,
    map: Mutex<Map>,
}

/// The non-owned version of a `Narc`.
#[derive(Debug)]
pub struct Weak<T> {
    inner: ArcWeak<Inner<T>>,
    id: Uid,
}

impl<T> Narc<T> {
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

    fn clone_at_site(&self, site: Site) -> Narc<T> {
        let mut map = self.inner.map.lock().unwrap();
        let parent_origin = map
            .strongs
            .get(&self.id)
            .expect("Internal consistency error (clone)")
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

    fn downgrade_at_site(this: &Self, site: Site) -> Weak<T> {
        let mut map = this.inner.map.lock().unwrap();
        // No need to `::remove` here because the strong ref will be dropped.
        let prev_origin = map
            .strongs
            .get(&this.id)
            .expect("Internal consistency error (downgrade)")
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

    /// Returns a new `Narc` with the provided file name and line as the 'origin'.
    pub fn new_at_line(data: T, file: &'static str, line: u32) -> Narc<T> {
        Narc::new_at_site(data, Site::SourceFile { file, line })
    }

    /// Creates a new [`Weak`][weak] pointer to this value.
    pub fn clone_at_line(&self, file: &'static str, line: u32) -> Narc<T> {
        self.clone_at_site(Site::SourceFile { file, line })
    }

    /// Creates a new [`Weak`][weak] pointer to this value.
    pub fn downgrade_at_line(this: &Self, file: &'static str, line: u32) -> Weak<T> {
        Narc::downgrade_at_site(this, Site::SourceFile { file, line })
    }

    // TODO: Come up with a clever way to impl this.
    //
    // /// Returns the contained value if the `Narc` has exactly one strong reference.
    // pub fn try_unwrap(this: Self) -> Result<T, Self> {
    //     Arc::try_unwrap(this.inner)
    //         .map(|i| i.data)
    //         .map_err(|i| Narc { inner: i })
    // }

    pub fn weak_count(this: &Narc<T>) -> usize {
        unimplemented!()
    }

    pub fn strong_count(this: &Narc<T>) -> usize {
        unimplemented!()
    }

    pub fn ptr_eq(this: &Narc<T>, other: &Narc<T>) -> bool {
        unimplemented!()
    }

    pub fn make_mut(this: &mut Narc<T>) -> &mut T {
        unimplemented!()
    }

    pub fn get_mut(this: &mut Narc<T>) -> Option<&mut T> {
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

impl<T> Weak<T> {
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
