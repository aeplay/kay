use compact::Compact;
use std::cell::Cell;

// TODO: make this much more simple and just like a Box once we can move out of messages!

/// A Marker for state of an actor instance that is not managed by the actor system.
/// As such it will not be compacted, **nor persisted**.
/// External implements clone, so it can be used in actor state, but **you have to ensure
/// at runtime** that only one actor or message ever holds onto an external.
/// Any attempt to access the external from two different places will throw (see `steal`).
pub struct External<T> {
    maybe_owned: Cell<Option<Box<T>>>,
}

impl<T> External<T> {
    /// Create a new `External` holding the given content
    pub fn new(content: T) -> Self {
        External {
            maybe_owned: Cell::new(Some(Box::new(content))),
        }
    }

    /// Create a new `External` directly from a `Box` holding the given content
    pub fn from_box(content: Box<T>) -> Self {
        External {
            maybe_owned: Cell::new(Some(content)),
        }
    }

    /// A more explicit way to clone an External,
    /// which effectively takes the held value out of the old external.
    /// Stealing or cloning twice from the same external will throw.
    pub fn steal(&self) -> Self {
        self.clone()
    }

    /// Take the content out of the external, as a `Box`.
    /// This, like stealing/cloning can only be done once.
    pub fn into_box(self) -> Box<T> {
        self.maybe_owned
            .into_inner()
            .expect("Tried to get Box from already taken external")
    }
}

// TODO: this is _really_ screwy, see above
impl<T> Clone for External<T> {
    fn clone(&self) -> Self {
        External {
            maybe_owned: Cell::new(Some(
                self.maybe_owned
                    .take()
                    .expect("Tried to clone already taken external"),
            )),
        }
    }
}

impl<T> ::std::ops::Deref for External<T> {
    type Target = T;

    fn deref(&self) -> &T {
        let option_ref = unsafe { &(*self.maybe_owned.as_ptr()) };
        &**option_ref
            .as_ref()
            .expect("Tried to deref already taken external")
    }
}

impl<T> ::std::ops::DerefMut for External<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self
            .maybe_owned
            .get_mut()
            .as_mut()
            .expect("Tried to mut deref already taken external")
    }
}

impl<T> Compact for External<T> {
    fn is_still_compact(&self) -> bool {
        true
    }
    fn dynamic_size_bytes(&self) -> usize {
        0
    }
    unsafe fn compact(source: *mut Self, dest: *mut Self, _new_dynamic_part: *mut u8) {
        ::std::ptr::copy_nonoverlapping(source, dest, 1)
    }
    unsafe fn decompact(source: *const Self) -> Self {
        ::std::ptr::read(source)
    }
}
