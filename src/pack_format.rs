use serde::{Serialize, Serializer};

/// Pack format representation that supports both integer and decimal formats
/// Minecraft 1.21.9+ introduced decimal pack formats (e.g., 88.0)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PackFormat {
    /// Integer format (e.g., 18, 48, 88)
    Integer(u8),
    /// Decimal format (e.g., 88.0)
    /// Represented as (major, minor) where format is "major.minor"
    Decimal(u8, u8),
}

impl PackFormat {
    /// Create from an integer
    pub fn from_int(value: u8) -> Self {
        PackFormat::Integer(value)
    }

    /// Create from a string like "88" or "88.0"
    pub fn from_str(s: &str) -> Result<Self, String> {
        if let Some(dot_pos) = s.find('.') {
            // Decimal format
            let major = s[..dot_pos]
                .parse::<u8>()
                .map_err(|_| format!("Invalid pack format: {}", s))?;
            let minor = s[dot_pos + 1..]
                .parse::<u8>()
                .map_err(|_| format!("Invalid pack format: {}", s))?;
            Ok(PackFormat::Decimal(major, minor))
        } else {
            // Integer format
            let value = s
                .parse::<u8>()
                .map_err(|_| format!("Invalid pack format: {}", s))?;
            Ok(PackFormat::Integer(value))
        }
    }

    /// Convert to JSON value (integer or decimal string)
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            PackFormat::Integer(v) => serde_json::Value::Number((*v).into()),
            PackFormat::Decimal(major, minor) => {
                serde_json::Value::String(format!("{}.{}", major, minor))
            }
        }
    }

    /// Get the major version number
    pub fn major(&self) -> u8 {
        match self {
            PackFormat::Integer(v) => *v,
            PackFormat::Decimal(major, _) => *major,
        }
    }
}

impl Default for PackFormat {
    fn default() -> Self {
        // Default to pack format 18 for maximum compatibility (Minecraft 1.20.2+)
        PackFormat::Integer(18)
    }
}

impl std::fmt::Display for PackFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackFormat::Integer(v) => write!(f, "{}", v),
            PackFormat::Decimal(major, minor) => write!(f, "{}.{}", major, minor),
        }
    }
}

impl Serialize for PackFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            PackFormat::Integer(v) => serializer.serialize_u8(*v),
            PackFormat::Decimal(major, minor) => {
                serializer.serialize_str(&format!("{}.{}", major, minor))
            }
        }
    }
}
