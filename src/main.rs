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

struct RefData {}

#[derive(Debug)]
struct Holder<T> {
    data: Option<Box<T>>,
    strong_refs: Mutex<Vec<RefId>>,
    weak_refs: Mutex<Vec<RefId>>,
    id_counter: Mutex<usize>,
}

impl<T> Holder<T> {
    #[inline]
    fn new(data: T) -> Holder<T> {
        Holder {
            data: Some(Box::new(data)),
            strong_refs: Mutex::new(Vec::new()),
            weak_refs: Mutex::new(Vec::new()),
            id_counter: Mutex::new(0),
        }
    }

    #[inline]
    fn new_floating(data: T) -> *const Holder<T> {
        let boxed = Box::new(Self::new(data));
        Box::into_raw(boxed) as *const Holder<T>
    }

    #[inline]
    fn create_strong_ref(&self, file: &'static str, line: usize) -> Strong<T> {
        let mut strong_refs = self
            .strong_refs
            .lock()
            .expect("poisoned Holder::strong_refs");

        // We perform this assert after the `strong_refs` lock, to hitch a ride on the lock.
        assert!(
            self.data.is_some(),
            "Cannot create strong reference, data has already been dropped.",
        );

        let mut id_counter = self
            .id_counter
            .lock()
            .expect("Poisoned Holder::id_counter.");

        // Get the next available ID and increment ID counter.
        let id = *id_counter;
        *id_counter += 1;

        let rid = RefId { file, line, id };
        strong_refs.push(rid);

        Strong {
            id: rid,
            holder: self as *const Holder<T>,
        }
    }

    #[inline]
    fn drop_strong_ref(ptr: *const Holder<T>, id: &RefId) {
        let holder = unsafe { &mut *(ptr as *mut Holder<T>) };

        assert!(
            holder.data.is_some(),
            "Tried dropping a strong ref, even though value has already been dropped."
        );

        let mut strong_refs = holder
            .strong_refs
            .lock()
            .expect("Poisoned Holder::strong_refs.");

        // Remove from list of strong_refs.
        let idx = strong_refs
            .iter()
            .position(|rid| rid == id)
            .expect("Could not find reference while dropping strong ref.");

        strong_refs.remove(idx);

        if strong_refs.is_empty() {
            // There are no more references to the value, we can now drop it.
            let data = holder.data.take();

            // Explicit.
            drop(data);
        }

        // `strong_refs` is released here.
    }

    #[inline]
    fn drop_weak_ref(ptr: *const Holder<T>, id: &RefId) {
        let holder = unsafe { &mut *(ptr as *mut Holder<T>) };

        // We lock `strong_refs` as well, to preserve locking order.
        let mut _strong_refs = holder
            .strong_refs
            .lock()
            .expect("Poisoned Holder::strong_refs.");

        let weak_refs = holder
            .weak_refs()
            .lock()
            .expect("poisoned Holder::strong_refs");
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
