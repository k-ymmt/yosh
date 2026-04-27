//! Macro that wires a user-implemented `Plugin` into the WIT bindings.

/// Generate the WIT exports (`yosh:plugin/plugin` and `yosh:plugin/hooks`)
/// from a `Plugin` implementation. Place at the crate root.
///
/// # Example
///
/// ```ignore
/// #[derive(Default)]
/// struct MyPlugin;
///
/// impl yosh_plugin_sdk::Plugin for MyPlugin {
///     fn commands(&self) -> &[&'static str] { &["hello"] }
///     fn exec(&mut self, _cmd: &str, _args: &[String]) -> i32 { 0 }
/// }
///
/// yosh_plugin_sdk::export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export {
    ($plugin_type:ty) => {
        static __YOSH_PLUGIN_INSTANCE: ::std::sync::Mutex<Option<$plugin_type>>
            = ::std::sync::Mutex::new(None);

        fn __yosh_plugin_instance_get<R>(
            f: impl FnOnce(&mut $plugin_type) -> R,
        ) -> R {
            let mut guard = __YOSH_PLUGIN_INSTANCE.lock()
                .unwrap_or_else(|e| e.into_inner());
            if guard.is_none() {
                *guard = Some(<$plugin_type as ::core::default::Default>::default());
            }
            f(guard.as_mut().expect("plugin instance present"))
        }

        struct __YoshPluginExports;

        impl $crate::plugin_iface::Guest for __YoshPluginExports {
            fn metadata() -> $crate::PluginInfo {
                __yosh_plugin_instance_get(|p| {
                    let commands = $crate::Plugin::commands(p)
                        .iter().map(|s| (*s).to_string()).collect();
                    let required_capabilities = $crate::Plugin::required_capabilities(p)
                        .iter().map(|c| c.as_str().to_string()).collect();
                    let implemented_hooks = $crate::Plugin::implemented_hooks(p)
                        .iter().copied().collect();
                    $crate::PluginInfo {
                        name: env!("CARGO_PKG_NAME").to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        commands,
                        required_capabilities,
                        implemented_hooks,
                    }
                })
            }

            fn on_load() -> Result<(), String> {
                __yosh_plugin_instance_get(|p| $crate::Plugin::on_load(p))
            }

            fn exec(command: String, args: Vec<String>) -> i32 {
                __yosh_plugin_instance_get(|p| $crate::Plugin::exec(p, &command, &args))
            }

            fn on_unload() {
                __yosh_plugin_instance_get(|p| $crate::Plugin::on_unload(p));
            }
        }

        impl $crate::hooks_iface::Guest for __YoshPluginExports {
            fn pre_exec(command: String) {
                __yosh_plugin_instance_get(|p| $crate::Plugin::hook_pre_exec(p, &command));
            }
            fn post_exec(command: String, exit_code: i32) {
                __yosh_plugin_instance_get(|p|
                    $crate::Plugin::hook_post_exec(p, &command, exit_code));
            }
            fn on_cd(old_dir: String, new_dir: String) {
                __yosh_plugin_instance_get(|p|
                    $crate::Plugin::hook_on_cd(p, &old_dir, &new_dir));
            }
            fn pre_prompt() {
                __yosh_plugin_instance_get(|p| $crate::Plugin::hook_pre_prompt(p));
            }
        }

        // Register the export struct with the WIT-generated world.
        // The SDK re-exports the WIT-generated export macro under
        // `export_wit_bindings` (via `export_macro_name` in generate!) to avoid
        // a name collision with this user-facing `export!` macro.
        $crate::export_wit_bindings!(__YoshPluginExports with_types_in $crate);
    };
}
