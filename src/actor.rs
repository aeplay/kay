use compact::Compact;
use id::{RawID, TypedID};
use storage_aware::StorageAware;

/// Represents both Actors and ActorTraits, used everywhere both are admissible
pub trait ActorOrActorTrait: 'static {
    /// The unique `TypedID` of this actor or actor trait
    type ID: TypedID;
}

impl<A: Actor> ActorOrActorTrait for A {
    type ID = <Self as Actor>::ID;
}

/// Trait that Actors instance have to implement for a [`InstanceStore`](struct.InstanceStore.html)
/// so their internally stored instance `RawID` can be gotten and set.
///
/// Furthermore, an `Actor` has to implement [`Compact`](../../compact), so a `InstanceStore`
/// can compactly store each `Actor`'s potentially dynamically-sized state.
///
/// This trait can is auto-derived when using the
/// [`kay_codegen`](../../kay_codegen/index.html) build script.
pub trait Actor: Compact + StorageAware + 'static {
    /// The unique `TypedID` of this actor
    type ID: TypedID;
    /// Get `TypedID` of this actor
    fn id(&self) -> Self::ID;
    /// Set the full RawID (Actor type id + instance id)
    /// of this actor (only used internally by `InstanceStore`)
    unsafe fn set_id(&mut self, id: RawID);

    /// Get the id of this actor as an actor trait `TypedID`
    /// (available if the actor implements the corresponding trait)
    fn id_as<TargetID: TraitIDFrom<Self>>(&self) -> TargetID {
        TargetID::from(self.id())
    }
}

/// Helper trait that signifies that an actor's `TypedID` can be converted
/// to an actor trait `TypedID` if that actor implements the corresponding trait.
pub trait TraitIDFrom<A: Actor>: TypedID {
    /// Construct the actor trait `TypedID` from an actor's `TypedID`
    fn from(id: <A as Actor>::ID) -> Self {
        Self::from_raw(id.as_raw())
    }
}
