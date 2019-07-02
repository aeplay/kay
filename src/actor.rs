use compact::Compact;
use crate::id::{RawID, TypedID};
use crate::storage_aware::StorageAware;

/// Represents an adressable entitiy in the system:
/// Either an actor class or an actor trait.
pub trait ActorOrActorTrait: 'static {
    /// The TypedID used to refer to instances of this actor class or actor trait
    type ID: TypedID;
}

impl<A: Actor> ActorOrActorTrait for A {
    type ID = <Self as Actor>::ID;
}

/// Must be implemented by every struct to be used
/// as actor instance state
pub trait Actor: Compact + StorageAware + 'static {
    /// The TypedID used to refer to instances of this actor class
    type ID: TypedID;
    /// Get the ID referring to this actor instace
    fn id(&self) -> Self::ID;
    /// Set the ID of this actor instance - only to be used by the system itself
    unsafe fn set_id(&mut self, id: RawID);

    /// Convert the ID of this instance into an actor trait ID.
    /// Only works if the actor implements the corresponding actor trait
    fn id_as<TargetID: TraitIDFrom<Self>>(&self) -> TargetID {
        TargetID::from(self.id())
    }
}

/// A marker that an actor implements a trait and thus
/// its ID can be converted to the corresponding actor trait ID
pub trait TraitIDFrom<A: Actor>: TypedID {
    /// Convert from actor ID to trait ID
    fn from(id: <A as Actor>::ID) -> Self {
        Self::from_raw(id.as_raw())
    }
}
