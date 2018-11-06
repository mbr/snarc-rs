use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub struct RefId {
    file: &'static str,
    line: usize,
    id: usize,
}

impl RefId {
    pub fn file(&self) -> &'static str {
        self.file
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn id(&self) -> usize {
        self.id
    }
}

#[derive(Debug)]
pub struct Strong<T> {
    id: RefId,
    holder: *const Holder<T>,
}

#[derive(Debug)]
pub struct Weak<T> {
    id: RefId,
    holder: *const Holder<T>,
}

#[derive(Debug)]
struct MetaData {
    strong_refs: Vec<RefId>,
    weak_refs: Vec<RefId>,
    id_counter: usize,
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

    fn new_floating(data: T) -> *const Holder<T> {
        let boxed = Box::new(Self::new(data));
        Box::into_raw(boxed) as *const Holder<T>
    }

    fn create_strong_ref(&self, file: &'static str, line: usize) -> Strong<T> {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        // We perform this assert after the `strong_refs` lock, to hitch a ride on the lock.
        // If we ever run into this assert, some invariant we assumed did not hold.
        assert!(
            self.data.is_some(),
            "Cannot create strong reference, data has already been dropped. This is a bug",
        );

        // Get the next available ID and increment ID counter, to create the new reference id.
        let id = meta.id_counter;
        meta.id_counter += 1;
        let rid = RefId { file, line, id };

        // We can now store this ID and return the reference.
        meta.strong_refs.push(rid);
        Strong {
            id: rid,
            holder: self as *const Holder<T>,
        }
    }

    fn create_weak_ref(&self, file: &'static str, line: usize) -> Weak<T> {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        let id = meta.id_counter;
        meta.id_counter += 1;
        let rid = RefId { file, line, id };
        meta.weak_refs.push(rid);
        Weak {
            id: rid,
            holder: self as *const Holder<T>,
        }
    }

    fn drop_strong_ref(&self, id: &RefId) {
        let mut meta = self.meta.lock().expect("Poisoned metadata");
        assert!(
            self.data.is_some(),
            "Tried dropping a strong ref, even though the data has already been dropped."
        );

        // Remove from list of strong_refs.
        delete_from_vec(&mut meta.strong_refs, id);

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

        // `meta` is released here.

        // FIXME: Clean up remaining memory (free holder).
    }

    fn drop_weak_ref(&self, id: &RefId) {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        delete_from_vec(&mut meta.weak_refs, id);

        // FIXME: Clean up remaining memory (free holder).
    }
}

impl<T> Strong<T> {
    #[inline]
    pub fn new_anonymous(data: T) -> Strong<T> {
        Self::new(data, "<no location available>", 0)
    }

    #[inline]
    pub fn new(data: T, file: &'static str, line: usize) -> Strong<T> {
        let holder = Box::leak(Box::new(Holder::new(data)));
        holder.create_strong_ref(file, line)
    }

    pub fn clone(&self, file: &'static str, line: usize) -> Strong<T> {
        let holder = unsafe { &*self.holder };
        holder.create_strong_ref(file, line)
    }
}

impl<T> Clone for Strong<T> {
    fn clone(&self) -> Self {
        self.clone("<from clone>", 0)
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
        unsafe { &*self.holder }.drop_strong_ref(&self.id);
    }
}

impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        unsafe { &*self.holder }.drop_weak_ref(&self.id);
    }
}

fn main() {
    println!("Hello, world!");
}
