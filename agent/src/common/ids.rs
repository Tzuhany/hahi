// ============================================================================
// Typed ID Wrappers
#![allow(dead_code)]
//
// Opaque string IDs for domain entities. Newtypes prevent mixing a ThreadId
// with a MessageId at the type level — compile-time safety at zero runtime cost.
//
// Each type:
//   - Wraps a String (cheap clone, zero-copy borrow via as_str())
//   - Generates fresh UUIDs via ::new()
//   - Serialises/deserialises transparently (just the inner string)
//   - Implements Display for use in tracing spans and log messages
//
// Adoption is gradual: new code uses these types, old code keeps plain &str/String
// until it touches a layer that now speaks typed IDs.
// ============================================================================

/// Stamp out a typed ID newtype.
///
/// Generates: `new()`, `from_string()`, `as_str()`, `Default`, `Display`,
/// `From<String>`, `From<&str>`, `Serialize`, `Deserialize`, `Hash`, `Eq`.
#[macro_export]
macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub String);

        impl $name {
            /// Generate a fresh random UUID-backed ID.
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            /// Wrap an existing string value.
            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

typed_id!(ThreadId);
typed_id!(AgentId);
typed_id!(MessageId);
typed_id!(MemoryId);
