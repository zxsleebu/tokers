//! Whose device ids a request carries.

use std::path::Path;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// `Template` keeps the device_id/iid of the captured request. It is the proven
/// identity: general/video search reject anything else. `Device` overrides both
/// ids; random unregistered ids pass with the default-key signature, while a
/// freshly app-registered device does not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Identity {
    #[default]
    Template,
    Device {
        device_id: String,
        iid: String,
    },
}

impl Identity {
    pub fn device(device_id: impl Into<String>, iid: impl Into<String>) -> Self {
        Identity::Device { device_id: device_id.into(), iid: iid.into() }
    }

    /// Fresh random ids in the range real ones fall in.
    pub fn random() -> Self {
        let mut rng = rand::rng();
        Identity::Device {
            device_id: rng.random_range(7_000_000_000_000_000_000u64..8_000_000_000_000_000_000).to_string(),
            iid: rng.random_range(7_000_000_000_000_000_000u64..8_000_000_000_000_000_000).to_string(),
        }
    }

    /// `device_id` given, iid random (19 digits).
    pub fn with_device_id(device_id: impl Into<String>) -> Self {
        let mut rng = rand::rng();
        let iid: String = (0..19).map(|_| char::from(b'0' + rng.random_range(0..10u8))).collect();
        Identity::Device { device_id: device_id.into(), iid }
    }

    /// A device JSON with `device_id` and `iid` fields (other fields ignored).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        #[derive(Deserialize)]
        struct DeviceFile {
            #[serde(deserialize_with = "crate::de::string")]
            device_id: String,
            #[serde(default, alias = "install_id", deserialize_with = "crate::de::string")]
            iid: String,
        }
        let raw = std::fs::read_to_string(path.as_ref())?;
        let file: DeviceFile = serde_json::from_str(&raw)
            .map_err(|e| Error::InvalidArgument(format!("{}: {e}", path.as_ref().display())))?;
        Ok(if file.iid.is_empty() {
            Self::with_device_id(file.device_id)
        } else {
            Identity::Device { device_id: file.device_id, iid: file.iid }
        })
    }

    pub fn ids(&self) -> Option<(&str, &str)> {
        match self {
            Identity::Template => None,
            Identity::Device { device_id, iid } => Some((device_id, iid)),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Identity::Template => "template",
            Identity::Device { device_id, .. } => device_id,
        }
    }
}
