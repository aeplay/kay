use compact::Compact;
use crate::id::{RawID, TypedID};
use crate::storage_aware::StorageAware;

pub trait ActorOrActorTrait: 'static {
    type ID: TypedID;
}

impl<A: Actor> ActorOrActorTrait for A {
    type ID = <Self as Actor>::ID;
}

pub trait Actor: Compact + StorageAware + 'static {
    type ID: TypedID;
    fn id(&self) -> Self::ID;
    unsafe fn set_id(&mut self, id: RawID);

    fn id_as<TargetID: TraitIDFrom<Self>>(&self) -> TargetID {
        TargetID::from(self.id())
    }
}

pub trait TraitIDFrom<A: Actor>: TypedID {
    fn from(id: <A as Actor>::ID) -> Self {
        Self::from_raw(id.as_raw())
    }
}
