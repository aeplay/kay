use crate::type_registry::ShortTypeId;
use crate::actor_system::World;
use crate::actor::ActorOrActorTrait;

/// Represents an `ActorSystem` in a networking topology
#[cfg_attr(
    feature = "serde-serialization",
    derive(Serialize, Deserialize)
)]
#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Debug)]
pub struct MachineID(pub u8);

/// A raw (untyped) ID referring to an actor class instance
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct RawID {
    /// instance ID within the class
    pub instance_id: u32,
    /// actual type of the instance (always points to a concrete class
    /// even if this `RawID` is contained in a `TypedID` representing
    /// an actor trait)
    pub type_id: ShortTypeId,
    /// The machine in the networking topology this instance lives on
    pub machine: MachineID,
    /// A version of the ID to be able to  safelyreuse instance IDs
    /// after an actor dies.
    pub version: u8,
}

pub fn broadcast_instance_id() -> u32 {
    u32::max_value()
}

pub fn broadcast_machine_id() -> MachineID {
    MachineID(u8::max_value())
}

impl RawID {
    /// Create a new RawID from its parts
    pub fn new(type_id: ShortTypeId, instance_id: u32, machine: MachineID, version: u8) -> Self {
        RawID {
            type_id,
            machine,
            version,
            instance_id,
        }
    }

    /// Convert a given RawID into one that represents a local broadcast
    pub fn local_broadcast(&self) -> RawID {
        RawID {
            instance_id: broadcast_instance_id(),
            ..*self
        }
    }

    /// Convert a given RawID into one that represents a global broadcast
    pub fn global_broadcast(&self) -> RawID {
        RawID {
            machine: broadcast_machine_id(),
            ..self.local_broadcast()
        }
    }

    /// Check whether this RawID represents a (local || global) broadcast
    pub fn is_broadcast(&self) -> bool {
        self.instance_id == broadcast_instance_id()
    }

    /// Check whether this RawID represents a global broadcast
    pub fn is_global_broadcast(&self) -> bool {
        self.machine == broadcast_machine_id()
    }

    /// Get the canonical string format of a RawID
    pub fn format(&self, world: &mut World) -> String {
        format!(
            "{}_{:X}.{:X}@{:X}",
            world.get_actor_name(self.type_id),
            self.instance_id,
            self.version,
            self.machine.0
        )
    }
}

impl ::std::fmt::Debug for RawID {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(
            f,
            "{:X}_{:X}.{:X}@{:X}",
            u16::from(self.type_id),
            self.instance_id,
            self.version,
            self.machine.0,
        )
    }
}

impl ::std::fmt::Display for RawID {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug)]
pub enum ParseRawIDError {
    Format,
    InvalidTypeId,
    ParseIntError(::std::num::ParseIntError),
}

impl ::std::fmt::Display for ParseRawIDError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Debug::fmt(self, f)
    }
}

impl ::std::str::FromStr for RawID {
    type Err = ParseRawIDError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(|c| c == '_' || c == '.' || c == '@');

        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(type_part), Some(instance_part), Some(version_part), Some(machine_part)) => {
                let type_id = ShortTypeId::new(
                    u16::from_str_radix(type_part, 16).map_err(ParseRawIDError::ParseIntError)?,
                ).ok_or(ParseRawIDError::InvalidTypeId)?;
                let instance_id = u32::from_str_radix(instance_part, 16)
                    .map_err(ParseRawIDError::ParseIntError)?;
                let version =
                    u8::from_str_radix(version_part, 16).map_err(ParseRawIDError::ParseIntError)?;
                let machine = MachineID(
                    u8::from_str_radix(machine_part, 16).map_err(ParseRawIDError::ParseIntError)?,
                );
                Ok(RawID {
                    type_id,
                    machine,
                    version,
                    instance_id,
                })
            }
            _ => Err(ParseRawIDError::Format),
        }
    }
}

#[cfg(feature = "serde-serialization")]
use std::marker::PhantomData;

#[cfg(feature = "serde-serialization")]
impl ::serde::ser::Serialize for RawID {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde-serialization")]
struct RawIDVisitor {
    marker: PhantomData<fn() -> RawID>,
}

#[cfg(feature = "serde-serialization")]
impl RawIDVisitor {
    fn new() -> Self {
        RawIDVisitor {
            marker: PhantomData,
        }
    }
}

#[cfg(feature = "serde-serialization")]
impl<'de> ::serde::de::Visitor<'de> for RawIDVisitor {
    type Value = RawID;

    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        formatter.write_str("A Raw Actor ID")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: ::serde::de::Error,
    {
        s.parse().map_err(::serde::de::Error::custom)
    }
}

#[cfg(feature = "serde-serialization")]
impl<'de> ::serde::de::Deserialize<'de> for RawID {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(RawIDVisitor::new())
    }
}

/// Wraps a `RawID`, bringing type information regarding the referenced
/// actor class or trait to compile time, for type safe handling of ids.
pub trait TypedID: Copy + Clone + Sized + ::std::fmt::Debug + ::std::hash::Hash {
    /// The actor class or actor trait referenced by this ID type.
    type Target: ActorOrActorTrait;

    /// Get the wrapped RawID
    fn as_raw(&self) -> RawID;
    /// Get the canonical string representation of the wrapped RawID
    fn as_raw_string(&self) -> String {
        self.as_raw().to_string()
    }
    /// Create a `TypedID` based on a `RawID`. Note: this should only be done
    /// when you are sure that the `RawID` actually points at the correct actor
    /// class or trait at runtime
    fn from_raw(raw: RawID) -> Self;
    /// Create a `TypedID` based on the canoncial string representation of a `RawID`.
    /// Note: this should only be done
    /// when you are sure that the `RawID` actually points at the correct actor
    /// class or trait at runtime
    fn from_raw_str(raw_str: &str) -> Result<Self, ParseRawIDError> {
        Ok(Self::from_raw(raw_str.parse()?))
    }

    /// Get the local first actor instance of type `Target`
    fn local_first(world: &mut World) -> Self {
        Self::from_raw(world.local_first::<Self::Target>())
    }

    /// Get the global first actor instance of type `Target`
    fn global_first(world: &mut World) -> Self {
        Self::from_raw(world.global_first::<Self::Target>())
    }

    /// Get an ID representing a local broadcast to actors of type `Target`
    fn local_broadcast(world: &mut World) -> Self {
        Self::from_raw(world.local_broadcast::<Self::Target>())
    }

    /// Get an ID representing a global broadcast to actors of type `Target`
    fn global_broadcast(world: &mut World) -> Self {
        Self::from_raw(world.global_broadcast::<Self::Target>())
    }
}
