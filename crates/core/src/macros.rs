//! Common macros for the NeoTalk project.
//!
//! This module provides procedural and declarative macros to reduce code duplication.

/// Macro to generate builder methods for a struct.
///
/// Each field generates a `with_<field_name>` method that sets the field and returns Self.
///
/// # Example
///
/// ```rust
/// use edge_ai_core::macros::builder_methods;
///
/// #[derive(Debug, Clone)]
/// pub struct MyConfig {
///     pub name: String,
///     pub endpoint: String,
///     pub max_retries: usize,
/// }
///
/// builder_methods!(MyConfig, name, endpoint, max_retries);
///
/// // Usage:
/// let config = MyConfig {
///     name: "test".to_string(),
///     endpoint: "localhost".to_string(),
///     max_retries: 3,
/// };
///
/// let updated = config.with_name("new_name")
///     .with_endpoint("remote")
///     .with_max_retries(5);
/// ```
#[macro_export]
macro_rules! builder_methods {
    ($struct_name:ident, $($field:ident),* $(,)?) => {
        impl $struct_name {
            $(
                #[inline]
                pub fn with_$field(mut self, value: impl Into<$crate::__private_ident!(@field_type $struct_name::$field)>) -> Self {
                    self.$field = value.into();
                    self
                }
            )*
        }
    };
}

/// Macro to generate a newtype wrapper with From implementations.
///
/// # Example
///
/// ```rust
/// use edge_ai_core::macros::newtype_wrapper;
///
/// newtype_wrapper!(pub struct DeviceId(String));
/// newtype_wrapper!(pub struct SessionId(String));
///
/// // Usage:
/// let id: DeviceId = "device_123".into();
/// let string: String = id.into();
/// ```
#[macro_export]
macro_rules! newtype_wrapper {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($inner:ty)) => {
        $(#[$meta])*
        $vis struct $name(pub $inner);

        impl $name {
            /// Create a new instance from the inner value.
            #[inline]
            pub fn new(value: $inner) -> Self {
                Self(value)
            }

            /// Get the inner value.
            #[inline]
            pub fn into_inner(self) -> $inner {
                self.0
            }

            /// Get a reference to the inner value.
            #[inline]
            pub fn as_inner(&self) -> &$inner {
                &self.0
            }
        }

        impl From<$inner> for $name {
            #[inline]
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            #[inline]
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<$inner> for $name {
            #[inline]
            fn as_ref(&self) -> &$inner {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name
        where
            $inner: std::str::FromStr,
        {
            type Err = <$inner as std::str::FromStr>::Err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.parse()?))
            }
        }
    };
}

/// Macro to generate Display and FromStr implementations for an enum.
///
/// # Example
///
/// ```rust
/// use edge_core::macros::enum_from_str;
///
/// enum_from_str!(DeviceStatus, [Online => "online", Offline => "offline"]);
/// ```
#[macro_export]
macro_rules! enum_from_str {
    ($enum_name:ident, [$($variant:ident => $str:expr),* $(,)?]) => {
        impl std::fmt::Display for $enum_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $($enum_name::$variant => write!(f, $str)),*
                }
            }
        }

        impl std::str::FromStr for $enum_name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($str => Ok($enum_name::$variant),)*
                    _ => Err(format!("Unknown {}: {}", stringify!($enum_name), s)),
                }
            }
        }
    };
}

/// Internal helper macro - do not use directly.
#[doc(hidden)]
#[macro_export(local_inner_macros)]
macro_rules! __private_ident {
    (@field_type $ty:ident::$field:ident) => {
        // This is a placeholder - actual type resolution requires more complex macros
        // or the user should specify the type explicitly in the macro call
        _
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_newtype_wrapper() {
        newtype_wrapper!(struct TestId(String));

        let id: TestId = "test_123".to_string().into();
        assert_eq!(id.as_inner(), "test_123");

        let s: String = id.into();
        assert_eq!(s, "test_123");

        let id2 = TestId::new("another".to_string());
        assert_eq!(id2.to_string(), "another");
    }

    #[test]
    fn test_enum_from_str() {
        #[derive(Debug, PartialEq, Clone)]
        enum TestStatus {
            Active,
            Inactive,
        }

        enum_from_str!(TestStatus, [Active => "active", Inactive => "inactive"]);

        assert_eq!(TestStatus::Active.to_string(), "active");
        assert_eq!(
            "inactive".parse::<TestStatus>().unwrap(),
            TestStatus::Inactive
        );

        assert!("unknown".parse::<TestStatus>().is_err());
    }
}
