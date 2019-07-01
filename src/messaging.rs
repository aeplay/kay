use super::compact::Compact;
use super::id::RawID;
use super::World;

pub enum Fate {
    Live,
    Die,
}

pub trait Message: Compact + 'static {}
impl<T: Compact + 'static> Message for T {}

pub type HandlerFnRef = dyn Fn(*mut(), *const (), &mut World) -> Fate;

#[derive(Compact, Clone)]
#[repr(C)]
pub struct Packet<M: Message> {
    pub recipient_id: RawID,
    pub message: M,
}
