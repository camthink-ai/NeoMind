//! Procedural and declarative macros for plugin development.

/// Macro to define a plugin descriptor
///
/// # Example
///
/// ```rust
/// use neotalk_plugin_sdk::prelude::*;
///
/// #[plugin_description]
/// fn my_plugin_descriptor() -> PluginDescriptor {
///     PluginDescriptor::new("my-plugin", "1.0.0")
///         .with_plugin_type(PluginType::Tool)
///         .with_name("My Plugin")
///         .with_description("A sample plugin")
/// }
/// ```
#[macro_export]
macro_rules! plugin_description {
    (
        $(#[$meta:meta])*
        fn $name:ident() -> $ty:ty {
            $($body:tt)*
        }
    ) => {
        $(#[$meta])*
        #[no_mangle]
        pub extern "C" fn $name() -> $ty {
            $($body)*
        }
    };
}

/// Macro to implement the plugin trait
///
/// # Example
///
/// ```rust
/// use neotalk_plugin_sdk::prelude::*;
///
/// struct MyPlugin;
///
/// #[plugin_impl]
/// impl MyPlugin {
///     fn new(config: &Value) -> PluginResult<Self> {
///         Ok(MyPlugin)
///     }
///
///     fn handle(&mut self, request: PluginRequest) -> PluginResult<PluginResponse> {
///         Ok(PluginResponse::success(json!({"status": "ok"})))
///     }
/// }
/// ```
#[macro_export]
macro_rules! plugin_impl {
    (
        impl $ty:ident {
            $($tt:tt)*
        }
    ) => {
        impl $ty {
            $($tt)*
        }
    };
}

/// Macro to export a plugin descriptor
///
/// This macro generates the necessary FFI exports for a plugin.
///
/// # Example
///
/// ```rust
/// use neotalk_plugin_sdk::prelude::*;
///
/// struct MyPlugin;
///
/// export_plugin!(MyPlugin, "my-plugin", "1.0.0", PluginType::Tool);
/// ```
#[macro_export]
macro_rules! export_plugin {
    (
        $ty:ty,
        $id:expr,
        $version:expr,
        $plugin_type:expr
    ) => {
        $crate::export_plugin! {
            $ty,
            $id,
            $version,
            $plugin_type,
            name: $id,
            description: "",
        }
    };
    (
        $ty:ty,
        $id:expr,
        $version:expr,
        $plugin_type:expr,
        name: $name:expr,
        description: $desc:expr
    ) => {
        // Static descriptor
        static DESCRIPTOR: $crate::descriptor::PluginDescriptor = {
            let mut desc = $crate::descriptor::PluginDescriptor::new($id, $version)
                .with_plugin_type($plugin_type)
                .with_name($name)
                .with_description($desc);

            // Set default capabilities
            desc = desc.with_capability($crate::descriptor::capabilities::ASYNC);
            desc = desc.with_capability($crate::descriptor::capabilities::THREAD_SAFE);

            desc
        };

        // Export the descriptor
        #[no_mangle]
        pub static neotalk_plugin_descriptor: $crate::descriptor::CPluginDescriptor = unsafe {
            DESCRIPTOR.export()
        };

        // Create function
        #[no_mangle]
        pub extern "C" fn neotalk_plugin_create(
            config_json: *const u8,
            config_len: usize,
        ) -> *mut () {
            use std::ptr;
            use std::slice;
            use std::str;

            // Parse config
            let config_str = if config_json.is_null() || config_len == 0 {
                "{}"
            } else {
                let slice = unsafe { slice::from_raw_parts(config_json, config_len) };
                match str::from_utf8(slice) {
                    Ok(s) => s,
                    Err(_) => return ptr::null_mut(),
                }
            };

            let config: serde_json::Value = match serde_json::from_str(config_str) {
                Ok(c) => c,
                Err(_) => return ptr::null_mut(),
            };

            // Create plugin instance
            // For this SDK, we store the config in a Box
            let boxed = Box::new(config);
            Box::leak(boxed) as *mut serde_json::Value as *mut ()
        }

        // Destroy function
        #[no_mangle]
        pub extern "C" fn neotalk_plugin_destroy(instance: *mut ()) {
            unsafe {
                let _ = Box::from_raw(instance as *mut serde_json::Value);
            }
        }
    };
}

/// Macro to build a descriptor with common fields
#[macro_export]
macro_rules! descriptor {
    (
        id: $id:expr,
        version: $version:expr,
        plugin_type: $plugin_type:expr,
        name: $name:expr,
        description: $desc:expr
    ) => {
        $crate::descriptor::PluginDescriptor::new($id, $version)
            .with_plugin_type($plugin_type)
            .with_name($name)
            .with_description($desc)
            .with_capability($crate::descriptor::capabilities::ASYNC)
            .with_capability($crate::descriptor::capabilities::THREAD_SAFE)
    };
    (
        id: $id:expr,
        version: $version:expr,
        plugin_type: $plugin_type:expr,
        name: $name:expr,
        description: $desc:expr,
        author: $author:expr
    ) => {
        $crate::descriptor::PluginDescriptor::new($id, $version)
            .with_plugin_type($plugin_type)
            .with_name($name)
            .with_description($desc)
            .with_author($author)
            .with_capability($crate::descriptor::capabilities::ASYNC)
            .with_capability($crate::descriptor::capabilities::THREAD_SAFE)
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macro_compilation() {
        // Test that macros compile
        let _ = descriptor! {
            id: "test",
            version: "1.0.0",
            plugin_type: crate::descriptor::PluginType::Tool,
            name: "Test",
            description: "Test plugin"
        };

        // Test that export_plugin macro compiles (in tests, we don't actually export)
        struct TestPlugin;
        let _ = std::marker::PhantomData::<TestPlugin>;
    }
}
