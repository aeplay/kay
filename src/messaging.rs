use super::compact::Compact;
use super::id::RawID;
use super::World;

/// The self-chosen fate of an actor instance it returns after handling a message
pub enum Fate {
    /// The actor should continue to live after handling this message
    Live,
    /// The actor should die and be deallocated after handling this message
    Die,
}

/// Must be implemented by everything that can be sent between actors
pub trait Message: Compact + 'static {}
impl<T: Compact + 'static> Message for T {}

pub type HandlerFnRef = dyn Fn(*mut(), *const (), &mut World) -> Fate;

#[derive(Compact, Clone)]
#[repr(C)]
/// A message plus a recipient
pub struct Packet<M: Message> {
    /// The recipient
    pub recipient_id: RawID,
    /// The message
    pub message: M,
}
