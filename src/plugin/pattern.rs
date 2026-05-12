//! Re-export of `yosh_plugin_api::pattern` so existing call sites
//! (`super::pattern::CommandPattern`) keep compiling unchanged. The
//! canonical implementation now lives in `crates/yosh-plugin-api`
//! so `yosh-plugin-manager` can use the matcher without depending on
//! the yosh binary crate.
pub use yosh_plugin_api::pattern::*;
