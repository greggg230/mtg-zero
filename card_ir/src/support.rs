//! Hand-written value types the generated code uses.

use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use std::fmt;

/// An integer count that may be unbounded: the IR spells infinity as the
/// string `"inf"` (`TargetSpec.count_max` for "any number of targets").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Count {
    Finite(i64),
    Infinite,
}

impl<'de> Deserialize<'de> for Count {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Count;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an integer or \"inf\"")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Count, E> {
                Ok(Count::Finite(v))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Count, E> {
                i64::try_from(v)
                    .map(Count::Finite)
                    .map_err(|_| E::custom("count out of range"))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Count, E> {
                if v.fract() == 0.0 && v.abs() < 9.0e15 {
                    Ok(Count::Finite(v as i64))
                } else {
                    Err(E::custom(format!("non-integral count {v}")))
                }
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Count, E> {
                match v {
                    "inf" => Ok(Count::Infinite),
                    _ => Err(E::custom(format!("unknown count {v:?}"))),
                }
            }
        }
        d.deserialize_any(V)
    }
}

/// A slot no encoded card has ever filled (its schema type is `never`):
/// deserializing a value for it is an error, so a parser change that
/// starts using the slot fails loudly until the schema is regenerated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Never {}

impl<'de> Deserialize<'de> for Never {
    fn deserialize<D: Deserializer<'de>>(_d: D) -> Result<Self, D::Error> {
        Err(de::Error::custom(
            "value in a slot the IR schema has never seen populated; regenerate the schema",
        ))
    }
}
