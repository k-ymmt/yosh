//! Command and hook dispatch into loaded plugins.

use wasmtime::Store;

use yosh_plugin_api::{CAP_HOOK_ON_CD, CAP_HOOK_POST_EXEC, CAP_HOOK_PRE_EXEC, CAP_HOOK_PRE_PROMPT};

use crate::env::ShellEnv;

use super::generated::PluginWorld;
use super::generated::yosh::plugin::types::HookName;
use super::host::HostContext;
use super::invoke::{log_entry_failure, log_with_env_failure, with_deadline, with_env};
use super::limits::PluginLimits;
use super::{PluginExec, PluginManager};

impl PluginManager {
    /// Dispatch a command name to the plugin layer.
    ///
    /// See `PluginExec` for the three-valued return semantics.
    pub fn exec_command(&mut self, env: &mut ShellEnv, name: &str, args: &[String]) -> PluginExec {
        let Some(idx) = self.plugins.iter().position(|p| p.provides_command(name)) else {
            return PluginExec::NotHandled;
        };
        let plugin = &mut self.plugins[idx];
        let ticks = plugin.limits.command_deadline_ticks();
        let timeout_ms = plugin.limits.command_timeout_ms;
        let max_mb = plugin.limits.max_memory_mb();
        match with_deadline(plugin, env, ticks, |bindings, store| {
            bindings.yosh_plugin_plugin().call_exec(store, name, args)
        }) {
            Ok(exit) => PluginExec::Handled(exit),
            Err(e) => {
                log_entry_failure(
                    &plugin.name,
                    &format!("command '{}'", name),
                    timeout_ms,
                    max_mb,
                    &e,
                );
                PluginExec::Failed
            }
        }
    }

    pub fn call_pre_exec(&mut self, env: &mut ShellEnv, cmd: &str) {
        self.dispatch_hook(
            env,
            CAP_HOOK_PRE_EXEC,
            HookName::PreExec,
            "pre_exec",
            hook_deadline,
            |bindings, store| bindings.yosh_plugin_hooks().call_pre_exec(store, cmd),
        );
    }

    pub fn call_post_exec(&mut self, env: &mut ShellEnv, cmd: &str, exit_code: i32) {
        self.dispatch_hook(
            env,
            CAP_HOOK_POST_EXEC,
            HookName::PostExec,
            "post_exec",
            hook_deadline,
            |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_post_exec(store, cmd, exit_code)
            },
        );
    }

    pub fn call_on_cd(&mut self, env: &mut ShellEnv, old_dir: &str, new_dir: &str) {
        self.dispatch_hook(
            env,
            CAP_HOOK_ON_CD,
            HookName::OnCd,
            "on_cd",
            hook_deadline,
            |bindings, store| {
                bindings
                    .yosh_plugin_hooks()
                    .call_on_cd(store, old_dir, new_dir)
            },
        );
    }

    pub fn call_pre_prompt(&mut self, env: &mut ShellEnv) {
        self.dispatch_hook(
            env,
            CAP_HOOK_PRE_PROMPT,
            HookName::PrePrompt,
            "pre_prompt",
            pre_prompt_deadline,
            |bindings, store| bindings.yosh_plugin_hooks().call_pre_prompt(store),
        );
    }

    /// Run one hook on every plugin that both holds `cap` and implements
    /// `hook`, in load order. `deadline` picks the plugin's epoch budget
    /// (ticks + the millisecond figure quoted in a timeout message);
    /// failures are logged under the `entry` name and never stop the
    /// remaining plugins.
    fn dispatch_hook(
        &mut self,
        env: &mut ShellEnv,
        cap: u32,
        hook: HookName,
        entry: &str,
        deadline: fn(&PluginLimits) -> (Option<u64>, u64),
        call: impl Fn(&PluginWorld, &mut Store<HostContext>) -> Result<(), wasmtime::Error>,
    ) {
        for plugin in &mut self.plugins {
            if plugin.capabilities & cap == 0 {
                continue;
            }
            if !plugin.implements_hook(hook) {
                continue;
            }
            let (ticks, timeout_ms) = deadline(&plugin.limits);
            let max_mb = plugin.limits.max_memory_mb();
            if let Err(e) = with_deadline(plugin, env, ticks, &call) {
                log_entry_failure(&plugin.name, entry, timeout_ms, max_mb, &e);
            }
        }
    }

    /// Call `on_unload` on every plugin and drop them. Best-effort: a trap
    /// in `on_unload` is logged and the plugin is dropped anyway.
    #[allow(dead_code)] // public manager API; called by host shutdown paths
    pub fn unload_all(&mut self, env: &mut ShellEnv) {
        // Drain so the borrow checker lets us call `with_env` on each.
        let mut plugins = std::mem::take(&mut self.plugins);
        for plugin in &mut plugins {
            if plugin.invalidated {
                continue;
            }
            if let Err(e) = with_env(plugin, env, |bindings, store| {
                bindings.yosh_plugin_plugin().call_on_unload(store)
            }) {
                log_with_env_failure(&plugin.name, &e, plugin.limits.max_memory_mb());
            }
        }
        // plugins drops here, releasing every Store and underlying instance.
        drop(plugins);
    }

    /// Check if any plugin provides the given command.
    #[allow(dead_code)] // public manager API; used by completion / dispatch lookups
    pub fn has_command(&self, name: &str) -> bool {
        self.plugins.iter().any(|p| p.provides_command(name))
    }

    /// True if any loaded plugin both holds the pre-exec or post-exec
    /// capability and implements the corresponding hook. Callers use this
    /// to skip building the joined command string passed to
    /// `call_pre_exec` / `call_post_exec` when no plugin would ever
    /// receive it (the common case: no plugins loaded, or none register
    /// these hooks).
    pub fn has_exec_hooks(&self) -> bool {
        self.plugins.iter().any(|p| {
            (p.capabilities & CAP_HOOK_PRE_EXEC != 0 && p.implements_hook(HookName::PreExec))
                || (p.capabilities & CAP_HOOK_POST_EXEC != 0
                    && p.implements_hook(HookName::PostExec))
        })
    }
}

/// Epoch budget for `pre_exec` / `post_exec` / `on_cd`: the plugin's
/// hook timeout (`None` ticks = unlimited).
fn hook_deadline(limits: &PluginLimits) -> (Option<u64>, u64) {
    (limits.hook_deadline_ticks(), limits.hook_timeout_ms)
}

/// Epoch budget for `pre_prompt`, which is always bounded.
fn pre_prompt_deadline(limits: &PluginLimits) -> (Option<u64>, u64) {
    (
        Some(limits.pre_prompt_ticks()),
        limits.pre_prompt_timeout_ms,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_exec_hooks_false_with_no_plugins_loaded() {
        // The common case this gate targets: no plugins loaded at all, so
        // callers should skip building the joined command string passed
        // to call_pre_exec / call_post_exec.
        let mgr = PluginManager::new();
        assert!(!mgr.has_exec_hooks());
    }
}
