use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
struct RefId {
    file: &'static str,
    line: usize,
    id: usize,
}

impl RefId {
    fn file(&self) -> &'static str {
        self.file
    }

    fn line(&self) -> usize {
        self.line
    }

    fn id(&self) -> usize {
        self.id
    }
}

#[derive(Debug)]
struct Strong<T> {
    id: RefId,
    holder: *const Holder<T>,
}

#[derive(Debug)]
struct Weak<T> {
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
    #[inline]
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

    #[inline]
    fn new_floating(data: T) -> *const Holder<T> {
        let boxed = Box::new(Self::new(data));
        Box::into_raw(boxed) as *const Holder<T>
    }

    #[inline]
    fn create_strong_ref(&self, file: &'static str, line: usize) -> Strong<T> {
        let mut meta = self.meta.lock().expect("Poisoned metadata");

        // We perform this assert after the `strong_refs` lock, to hitch a ride on the lock.
        assert!(
            self.data.is_some(),
            "Cannot create strong reference, data has already been dropped.",
        );

        // Get the next available ID and increment ID counter.
        let id = meta.id_counter;
        meta.id_counter += 1;

        let rid = RefId { file, line, id };
        meta.strong_refs.push(rid);

        Strong {
            id: rid,
            holder: self as *const Holder<T>,
        }
    }

    #[inline]
    fn drop_strong_ref(ptr: *const Holder<T>, id: &RefId) {
        let holder = unsafe { &mut *(ptr as *mut Holder<T>) };

        let mut meta = holder.meta.lock().expect("Poisoned metadata");
        assert!(
            holder.data.is_some(),
            "Tried dropping a strong ref, even though the data has already been dropped."
        );

        // Remove from list of strong_refs.
        delete_from_vec(&mut meta.strong_refs, id);

        if meta.strong_refs.is_empty() {
            // There are no more references to the value, we can now drop it.
            let data = holder.data.take();

            // Explicit.
            drop(data);
        }

        // `meta` is released here.
    }

    #[inline]
    fn drop_weak_ref(ptr: *const Holder<T>, id: &RefId) {
        let holder = unsafe { &mut *(ptr as *mut Holder<T>) };

        let mut meta = holder.meta.lock().expect("Poisoned metadata");

        // Remove from list of strong_refs.
        let idx = meta
            .strong_refs
            .iter()
            .position(|rid| rid == id)
            .expect("Could not find reference while dropping strong ref.");

        meta.strong_refs.remove(idx);
    }
}

impl<T> Strong<T> {
    #[inline]
    fn new_anonymous(data: T) -> Strong<T> {
        Self::new(data, "<no location available>", 0)
    }

    #[inline]
    fn new(data: T, file: &'static str, line: usize) -> Strong<T> {
        let holder = Box::leak(Box::new(Holder::new(data)));
        holder.create_strong_ref(file, line)
    }

    fn clone(&self, file: &'static str, line: usize) -> Strong<T> {
        let holder = unsafe { &*self.holder };
        holder.create_strong_ref(file, line)
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
        Holder::drop_strong_ref(self.holder, &self.id)
    }
}

// impl<T> Drop for Weak<T> {
//     fn drop(&mut self) {
//         Holder::drop_weak_ref(self.holder, &self.id)
//     }
// }

fn main() {
    println!("Hello, world!");
}
