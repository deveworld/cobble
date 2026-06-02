use serde::{Serialize, Serializer};

pub const SUPPORTED_MINECRAFT_VERSION: &str = "26.1.2";
pub const SUPPORTED_PACK_FORMAT: PackFormat = PackFormat::Decimal(101, 1);

/// Pack format representation that supports both integer and decimal formats
/// Minecraft 1.21.9+ introduced decimal pack formats (e.g., 88.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PackFormat {
    /// Integer format used by older Minecraft releases (e.g., 18, 48, 88).
    Integer(u8),
    /// Decimal format used by newer Minecraft releases (e.g., 88.0, 101.1).
    /// Represented as (major, minor) where format is "major.minor"
    Decimal(u8, u8),
}

impl PackFormat {
    /// Create from an integer
    pub fn from_int(value: u8) -> Self {
        PackFormat::Integer(value)
    }

    /// Create from a string like "101" or "101.1"
    pub fn parse_format(s: &str) -> Result<Self, String> {
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

    /// Convert to JSON value (integer or decimal number)
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            PackFormat::Integer(v) => serde_json::Value::Number((*v).into()),
            PackFormat::Decimal(major, minor) => {
                // Return as JSON number (float), not string
                let value_str = format!("{}.{}", major, minor);
                let value: f64 = value_str.parse().unwrap();
                serde_json::json!(value)
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

    /// Whether this pack format is supported by this Cobble release.
    pub fn is_supported(self) -> bool {
        self == SUPPORTED_PACK_FORMAT
    }
}

impl Default for PackFormat {
    fn default() -> Self {
        // Default to pack format 101.1 (Minecraft Java Edition 26.1.2)
        SUPPORTED_PACK_FORMAT
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
                // Serialize as JSON number (float), not string
                // Example: Decimal(101, 1) -> 101.1 (number), not "101.1" (string)
                let value_str = format!("{}.{}", major, minor);
                let value: f64 = value_str.parse().unwrap();
                serializer.serialize_f64(value)
            }
        }
    }
}
