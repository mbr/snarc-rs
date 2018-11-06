use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;

#[derive(Debug, Copy, Clone)]
pub enum Site {
    SourceFile { file: &'static str, line: usize },
    Unknown,
    // TODO: Backtrace,
}

#[derive(Debug, Clone)]
enum OriginKind {
    Created,
    ClonedFrom(usize, Box<Origin>),
    UpgradedFrom(usize, Box<Origin>),
    DowngradedFrom(usize, Box<Origin>),
}

#[derive(Debug, Clone)]
pub struct Origin {
    kind: OriginKind,
    site: Site,
}

#[derive(Debug)]
pub struct Strong<T> {
    id: usize,
    origin: Origin,
    holder: *const Holder<T>,
}

#[derive(Debug)]
pub struct Weak<T> {
    id: usize,
    origin: Origin,
    holder: *const Holder<T>,
}

#[derive(Debug)]
struct MetaData {
    strong_refs: Vec<usize>,
    weak_refs: Vec<usize>,
    id_counter: usize,
}

impl MetaData {
    fn should_cleanup(&self) -> bool {
        self.strong_refs.is_empty() && self.weak_refs.is_empty()
    }
}

#[derive(Debug)]
struct Holder<T> {
    data: Option<Box<T>>,
    meta: Mutex<MetaData>,
}

fn delete_from_vec<'a, T>(v: &mut Vec<T>, item: &T)
where
    T: fmt::Debug,
    T: PartialEq,
{
    match v.iter().position(|i| i == item) {
        None => panic!(
            "Tried to delete {:?} from vector {:?}, but did not find it.",
            item, v
        ),
        Some(idx) => {
            v.remove(idx);
        }
    }
}

impl<T> Holder<T> {
    fn new(data: T) -> Holder<T> {
        Holder {
            data: Some(Box::new(data)),
            meta: Mutex::new(MetaData {
                strong_refs: Vec::new(),
                weak_refs: Vec::new(),
                id_counter: 0,
            }),
        }
    }

    fn create_strong_ref(&self) -> Option<usize> {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        // We perform this assert after the `strong_refs` lock, to hitch a ride on the lock.
        if self.data.is_none() {
            return None;
        }

        // Get the next available ID and increment ID counter, to create the new reference id.
        let id = meta.id_counter;

        // We can now store this ID and return the reference.
        meta.strong_refs.push(id);
        Some(id)
    }

    fn create_weak_ref(&self) -> usize {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        let id = meta.id_counter;
        meta.id_counter += 1;

        meta.weak_refs.push(id);
        id
    }

    fn drop_strong_ref(&self, id: usize) -> bool {
        let mut meta = self.meta.lock().expect("Poisoned metadata");
        assert!(
            self.data.is_some(),
            "Tried dropping a strong ref, even though the data has already been dropped."
        );

        // Remove from list of strong_refs.
        delete_from_vec(&mut meta.strong_refs, &id);

        if meta.strong_refs.is_empty() {
            // Here, we have to cheat the borrow checker: We know there are no strong references to
            // this holder anymore and all weak refs have to waiting on the `meta` lock. There is
            // also no other call to `drop_strong_ref` waiting, because those will remove themselves
            // from the reference list before reaching this line.

            // This rules out any change of cloning the value, the only chance for a new strong
            // ref to come into existance is updating a weak ref. Weak ref's lock first, then check
            // if the value is still present, so we should have all bases covered.

            // For this reason, we sneakily upgrade our ref to drop the value:
            let self_mut = unsafe { &mut *(self as *const Self as *mut Self) };

            // There are no more references to the value, we can now drop it.
            let data = self_mut.data.take();

            // Explicit.
            drop(data);
        }
        meta.should_cleanup()
    }

    fn drop_weak_ref(&self, id: usize) -> bool {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        delete_from_vec(&mut meta.weak_refs, &id);

        meta.should_cleanup()
    }
}

impl<T> Strong<T> {
    fn new_with_site(data: T, site: Site) -> Strong<T> {
        let holder = Box::leak(Box::new(Holder::new(data)));
        let id = holder
            .create_strong_ref()
            .expect("Cannot create strong reference, data has already been dropped. This is a bug");

        Strong {
            id,
            origin: Origin {
                site,
                kind: OriginKind::Created,
            },
            holder,
        }
    }

    #[inline]
    fn clone_with_site(&self, site: Site) -> Strong<T> {
        let holder = unsafe { &*self.holder };

        let new_id = holder
            .create_strong_ref()
            .expect("Cannot clone strong reference, data has already been dropped. This is a bug");

        Strong {
            id: new_id,
            holder,
            origin: Origin {
                kind: OriginKind::ClonedFrom(self.id, Box::new(self.origin.clone())),
                site: site,
            },
        }
    }

    #[inline]
    fn downgrade_with_site(this: &Self, site: Site) -> Weak<T> {
        // Create new weak reference first. Downgrading always works.
        let holder = unsafe { &*this.holder };

        let id = holder.create_weak_ref();
        Weak {
            holder,
            id,
            origin: Origin {
                kind: OriginKind::DowngradedFrom(this.id, Box::new(this.origin.clone())),
                site,
            },
        }
    }

    #[inline]
    pub fn new(data: T) -> Strong<T> {
        Self::new_with_site(data, Site::Unknown)
    }

    #[inline]
    pub fn new_from(data: T, file: &'static str, line: usize) -> Strong<T> {
        Self::new_with_site(data, Site::SourceFile { file, line })
    }

    #[inline]
    pub fn clone_from(&self, file: &'static str, line: usize) -> Strong<T> {
        self.clone_with_site(Site::SourceFile { file, line })
    }

    #[inline]
    pub fn downgrade_from(this: &Self, file: &'static str, line: usize) -> Weak<T> {
        Self::downgrade_with_site(this, Site::SourceFile { file, line })
    }

    #[inline]
    pub fn downgrade(this: &Self) -> Weak<T> {
        Self::downgrade_with_site(this, Site::Unknown)
    }
}

impl<T> Clone for Strong<T> {
    fn clone(&self) -> Self {
        self.clone_with_site(Site::Unknown)
    }
}

impl<T> Deref for Strong<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let holder = unsafe { &*self.holder };
        &holder.data.as_ref().expect(
            "Encountered a missing value inside a holder while dereferencing a strong ref. \
             This is a bug!",
        )
    }
}

impl<T> Drop for Strong<T> {
    fn drop(&mut self) {
        if unsafe { &*self.holder }.drop_strong_ref(self.id) {
            // We are the last reference, we can cleanup now.
            let holder = unsafe { Box::from_raw(self.holder as *mut Self) };
            drop(holder);
        }
    }
}

impl<T> Weak<T> {
    #[inline]
    fn upgrade_with_site(this: &Self, site: Site) -> Option<Strong<T>> {
        let holder = unsafe { &*this.holder };

        holder.create_strong_ref().map(|id| Strong {
            holder,
            id,
            origin: Origin {
                kind: OriginKind::UpgradedFrom(this.id, Box::new(this.origin.clone())),
                site,
            },
        })
    }

    #[inline]
    pub fn upgrade_from(this: &Self, file: &'static str, line: usize) -> Option<Strong<T>> {
        Self::upgrade_with_site(this, Site::SourceFile { file, line })
    }

    #[inline]
    pub fn upgrade(this: &Self) -> Option<Strong<T>> {
        Self::upgrade_with_site(this, Site::Unknown)
    }
}

impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        if unsafe { &*self.holder }.drop_weak_ref(self.id) {
            let holder = unsafe { Box::from_raw(self.holder as *mut Self) };
            drop(holder);
        }
    }
}
