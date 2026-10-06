//! Lenient field deserializers: the API mixes `"123"`/`123`, `0`/`false`, and
//! sends `null` for empty lists and objects.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// string | number | bool | null -> String
pub fn string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    })
}

/// number | numeric string | bool | null -> i64 (non-numeric -> 0)
pub fn i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(b) => b.into(),
        _ => 0,
    })
}

/// Like [`i64`], clamped at 0. For counts and cursors.
pub fn u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)).unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(b) => b.into(),
        _ => 0,
    })
}

/// bool | number | "true"/"1" | null -> bool
pub fn bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Bool(b) => b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => matches!(s.trim(), "true" | "1"),
        _ => false,
    })
}

/// `null` -> `T::default()`; otherwise the normal `T` deserializer.
pub fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}
