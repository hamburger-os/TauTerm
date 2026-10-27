use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Process-local monotonic connection identity, serialized without JavaScript integer loss.
/// Only SessionStore allocates a fresh value. Children share their root connection's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeId(pub(crate) u64);

impl Serialize for RuntimeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for RuntimeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let value = value.parse::<u64>().map_err(serde::de::Error::custom)?;
        if value == 0 {
            return Err(serde::de::Error::custom(
                "runtime identity must be positive",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RuntimeIdentity {
    pub session_id: String,
    pub runtime_id: RuntimeId,
}

impl RuntimeIdentity {
    pub fn for_session(&self, session_id: String) -> Self {
        Self {
            session_id,
            runtime_id: self.runtime_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_ids_round_trip_as_strings_without_javascript_precision_loss() {
        let id = RuntimeId(u64::MAX);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", u64::MAX));
        assert_eq!(serde_json::from_str::<RuntimeId>(&json).unwrap(), id);
        for invalid in ["0", "\"0\"", "1", "\"-1\"", "\"18446744073709551616\""] {
            assert!(
                serde_json::from_str::<RuntimeId>(invalid).is_err(),
                "{invalid}"
            );
        }
    }
}
