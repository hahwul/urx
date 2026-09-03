//! API-key resolution and the precedence rules around it.
//!
//! Keys can arrive from four places. The documented order, highest first, is
//! `CLI flag` > `environment variable` > provider-config file > main config
//! file. The first two are indistinguishable to everything downstream, so they
//! are folded together early by [`seed_api_keys_from_env`].

use crate::cli::Args;

/// Providers gated behind an API key, in catalog order.
///
/// The flag and environment-variable names are derived from the id
/// (`--<id>-api-key`, `URX_<ID>_API_KEY`), which is what keeps a new keyed
/// provider from needing three more hardcoded strings.
pub const KEYED_PROVIDER_IDS: [&str; 5] = ["vt", "urlscan", "zoomeye", "github", "bevigil"];

/// The environment variable urx reads keys for `id` from.
pub fn api_key_env_var(id: &str) -> String {
    format!("URX_{}_API_KEY", id.to_uppercase())
}

/// The command-line flag that supplies keys for `id`.
pub fn api_key_flag(id: &str) -> String {
    format!("--{id}-api-key")
}

/// Every API key urx resolved for this run, per provider.
#[derive(Debug, Default, Clone)]
pub struct ApiKeys {
    pub vt: Vec<String>,
    pub urlscan: Vec<String>,
    pub zoomeye: Vec<String>,
    pub github: Vec<String>,
    pub bevigil: Vec<String>,
}

impl ApiKeys {
    /// Merge each provider's CLI/config keys with its environment variable.
    ///
    /// Resolving all four together means the four-line incantation this
    /// replaces exists once instead of being repeated at every call site, where
    /// one copy could quietly fall behind.
    pub fn resolve(args: &Args) -> Self {
        Self {
            vt: parse_api_keys(args.vt_api_key.clone(), &api_key_env_var("vt")),
            urlscan: parse_api_keys(args.urlscan_api_key.clone(), &api_key_env_var("urlscan")),
            zoomeye: parse_api_keys(args.zoomeye_api_key.clone(), &api_key_env_var("zoomeye")),
            github: parse_api_keys(args.github_api_key.clone(), &api_key_env_var("github")),
            bevigil: parse_api_keys(args.bevigil_api_key.clone(), &api_key_env_var("bevigil")),
        }
    }

    /// The keys resolved for provider `id`, or an empty slice for a keyless one.
    pub fn for_provider(&self, id: &str) -> &[String] {
        match id {
            "vt" => &self.vt,
            "urlscan" => &self.urlscan,
            "zoomeye" => &self.zoomeye,
            "github" => &self.github,
            "bevigil" => &self.bevigil,
            _ => &[],
        }
    }
}

/// Parse a comma-separated key list out of `env_var_name`.
///
/// Blank entries are dropped so a trailing comma or a `KEY=` with nothing after
/// it doesn't produce an empty key that later reads as an auth failure.
fn parse_env_api_keys(env_var_name: &str) -> Vec<String> {
    std::env::var(env_var_name)
        .ok()
        .map(|env_keys| {
            env_keys
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Combine CLI-supplied keys with the environment variable's, keeping CLI keys
/// first (so rotation starts with what the user named explicitly) and dropping
/// duplicates.
pub fn parse_api_keys(cli_keys: Vec<String>, env_var_name: &str) -> Vec<String> {
    let mut all_keys = cli_keys;
    all_keys.extend(parse_env_api_keys(env_var_name));

    let mut seen = std::collections::HashSet::new();
    all_keys.retain(|key| seen.insert(key.clone()));
    all_keys
}

/// Which providers had a key supplied directly by the user — on the CLI or via
/// the environment — as opposed to by a config file.
///
/// Config layers consult this to avoid overwriting a key the user named
/// explicitly.
#[derive(Debug, Default, Clone, Copy)]
pub struct DirectKeySources {
    pub vt: bool,
    pub urlscan: bool,
    pub zoomeye: bool,
    pub github: bool,
    pub bevigil: bool,
}

/// Fill empty API-key args from their environment variables, and report which
/// providers ended up with a user-supplied key.
///
/// This must run *before* any config file is applied, which is also what makes
/// the return value trustworthy: at this point a non-empty field can only have
/// come from the CLI or from the environment.
pub fn seed_api_keys_from_env(args: &mut Args) -> DirectKeySources {
    fn seed(slot: &mut Vec<String>, id: &str) -> bool {
        if slot.is_empty() {
            *slot = parse_env_api_keys(&api_key_env_var(id));
        }
        !slot.is_empty()
    }

    DirectKeySources {
        vt: seed(&mut args.vt_api_key, "vt"),
        urlscan: seed(&mut args.urlscan_api_key, "urlscan"),
        zoomeye: seed(&mut args.zoomeye_api_key, "zoomeye"),
        github: seed(&mut args.github_api_key, "github"),
        bevigil: seed(&mut args.bevigil_api_key, "bevigil"),
    }
}

/// Add `provider_name` to the selection when a key for it is available and it
/// isn't already selected.
pub fn auto_enable_provider(
    providers_list: &mut Vec<String>,
    api_keys: &[String],
    provider_name: &str,
    verbose: bool,
    silent: bool,
) {
    if !api_keys.is_empty() && !providers_list.iter().any(|p| p == provider_name) {
        providers_list.push(provider_name.to_string());
        if verbose && !silent {
            println!("Auto-enabling {provider_name} provider because API key is provided");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliProvided;
    use crate::config::{self, Config};
    use crate::test_support::{env_mutex, EnvGuard};
    use clap::Parser;

    #[test]
    fn test_api_key_names_are_derived_from_the_provider_id() {
        assert_eq!(api_key_env_var("vt"), "URX_VT_API_KEY");
        assert_eq!(api_key_env_var("zoomeye"), "URX_ZOOMEYE_API_KEY");
        assert_eq!(api_key_flag("github"), "--github-api-key");
    }

    #[test]
    fn test_auto_enable_provider() {
        let mut providers_list = vec!["wayback".to_string(), "cc".to_string()];
        let api_keys = vec!["test_api_key".to_string()];

        // Should add vt to the list
        auto_enable_provider(&mut providers_list, &api_keys, "vt", false, false);
        assert!(providers_list.contains(&"vt".to_string()));
        assert_eq!(providers_list.len(), 3);

        // Calling again shouldn't add duplicates
        auto_enable_provider(&mut providers_list, &api_keys, "vt", false, false);
        assert_eq!(providers_list.len(), 3);

        // Empty API key should not add the provider
        let empty_keys: Vec<String> = vec![];
        auto_enable_provider(&mut providers_list, &empty_keys, "urlscan", false, false);
        assert!(!providers_list.contains(&"urlscan".to_string()));
        assert_eq!(providers_list.len(), 3);
    }

    #[test]
    fn test_auto_enable_providers_with_env_vars() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("URX_VT_API_KEY", "test_vt_key"),
            ("URX_URLSCAN_API_KEY", "test_urlscan_key"),
        ]);

        let args = Args::parse_from(["urx", "example.com"]);
        let keys = ApiKeys::resolve(&args);
        let mut providers_list = Vec::new();

        auto_enable_provider(&mut providers_list, &keys.vt, "vt", false, false);
        auto_enable_provider(&mut providers_list, &keys.urlscan, "urlscan", false, false);

        assert!(providers_list.contains(&"vt".to_string()));
        assert!(providers_list.contains(&"urlscan".to_string()));
        assert_eq!(providers_list.len(), 2);
    }

    #[test]
    fn test_parse_api_keys() {
        // CLI keys only
        let cli_keys = vec!["key1".to_string(), "key2".to_string()];
        let result = parse_api_keys(cli_keys, "NONEXISTENT_ENV_VAR");
        assert_eq!(result, vec!["key1", "key2"]);

        let _env_lock = env_mutex().lock().unwrap();

        // Environment keys only, with surrounding whitespace trimmed
        let _guard = EnvGuard::set(&[("TEST_API_KEYS", "env_key1,env_key2, env_key3 ")]);
        let result = parse_api_keys(vec![], "TEST_API_KEYS");
        assert_eq!(result, vec!["env_key1", "env_key2", "env_key3"]);
        drop(_guard);

        // CLI + environment: CLI keys come first
        let _guard = EnvGuard::set(&[("TEST_API_KEYS", "env_key1,env_key2")]);
        let result = parse_api_keys(vec!["cli_key1".to_string()], "TEST_API_KEYS");
        assert_eq!(result, vec!["cli_key1", "env_key1", "env_key2"]);
        drop(_guard);

        // Duplicates are removed, first occurrence wins
        let _guard = EnvGuard::set(&[("TEST_API_KEYS", "key1,key2")]);
        let cli_keys = vec!["key1".to_string(), "key3".to_string()];
        let result = parse_api_keys(cli_keys, "TEST_API_KEYS");
        assert_eq!(result, vec!["key1", "key3", "key2"]);
        drop(_guard);

        // Empty entries are filtered out
        let _guard = EnvGuard::set(&[("TEST_API_KEYS", "key1,,key2, ,key3")]);
        let result = parse_api_keys(vec![], "TEST_API_KEYS");
        assert_eq!(result, vec!["key1", "key2", "key3"]);
    }

    #[test]
    fn test_multiple_api_keys_integration() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&["URX_VT_API_KEY", "URX_URLSCAN_API_KEY"]);

        let args = Args::parse_from([
            "urx",
            "example.com",
            "--vt-api-key",
            "vt_key1",
            "--vt-api-key",
            "vt_key2",
            "--urlscan-api-key",
            "url_key1",
        ]);

        assert_eq!(args.vt_api_key, vec!["vt_key1", "vt_key2"]);
        assert_eq!(args.urlscan_api_key, vec!["url_key1"]);

        let keys = ApiKeys::resolve(&args);
        assert_eq!(keys.vt, vec!["vt_key1", "vt_key2"]);
        assert_eq!(keys.urlscan, vec!["url_key1"]);
    }

    #[test]
    fn test_api_key_precedence() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env_vt_key")]);

        // An explicit CLI key sorts ahead of the environment's.
        let args = Args::parse_from(["urx", "example.com", "--vt-api-key", "arg_vt_key"]);
        let keys = ApiKeys::resolve(&args);
        assert_eq!(keys.vt, vec!["arg_vt_key", "env_vt_key"]);
        assert_eq!(keys.vt[0], "arg_vt_key");

        // Without one, the environment variable is the fallback.
        let args = Args::parse_from(["urx", "example.com"]);
        assert_eq!(ApiKeys::resolve(&args).vt, vec!["env_vt_key"]);
    }

    #[test]
    fn test_for_provider_maps_ids_and_ignores_keyless_ones() {
        let keys = ApiKeys {
            vt: vec!["v".to_string()],
            urlscan: vec!["u".to_string()],
            zoomeye: vec!["z".to_string()],
            github: vec!["g".to_string()],
            bevigil: vec!["b".to_string()],
        };
        for id in KEYED_PROVIDER_IDS {
            assert_eq!(
                keys.for_provider(id).len(),
                1,
                "{id} should map to its keys"
            );
        }
        assert!(keys.for_provider("wayback").is_empty());
    }

    #[test]
    fn test_env_api_keys_override_config_layers() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("URX_VT_API_KEY", "env-vt-1,env-vt-2"),
            ("URX_URLSCAN_API_KEY", "env-urlscan"),
            ("URX_ZOOMEYE_API_KEY", "env-zoomeye"),
        ]);
        let _github_guard = EnvGuard::unset(&["URX_GITHUB_API_KEY", "URX_BEVIGIL_API_KEY"]);

        let mut args = Args::parse_from(["urx", "example.com"]);
        let direct = seed_api_keys_from_env(&mut args);
        assert!(direct.vt && direct.urlscan && direct.zoomeye);
        assert!(!direct.github, "no GITHUB key was supplied");
        assert!(!direct.bevigil, "no BEVIGIL key was supplied");

        let mut config = Config::default();
        config.provider.vt_api_key = Some("config-vt".to_string());
        config.provider.urlscan_api_key = Some("config-urlscan".to_string());
        config.provider.zoomeye_api_key = Some("config-zoomeye".to_string());
        config.apply_to_args(&mut args, &CliProvided::default());

        let provider_keys = config::ProviderKeysConfig {
            vt_api_key: Some("provider-vt".to_string()),
            urlscan_api_key: Some("provider-urlscan".to_string()),
            zoomeye_api_key: Some("provider-zoomeye".to_string()),
            github_api_key: None,
            bevigil_api_key: None,
            notify_url: None,
            unknown: Default::default(),
        };
        provider_keys.apply_to_args(
            &mut args,
            config::CliSuppliedKeys {
                vt: direct.vt,
                urlscan: direct.urlscan,
                zoomeye: direct.zoomeye,
                github: direct.github,
                bevigil: direct.bevigil,
                notify: false,
            },
        );

        assert_eq!(args.vt_api_key, vec!["env-vt-1", "env-vt-2"]);
        assert_eq!(args.urlscan_api_key, vec!["env-urlscan"]);
        assert_eq!(args.zoomeye_api_key, vec!["env-zoomeye"]);
    }

    #[test]
    fn test_seed_api_keys_leaves_cli_values_alone() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env-vt")]);

        let mut args = Args::parse_from(["urx", "example.com", "--vt-api-key", "cli-vt"]);
        let direct = seed_api_keys_from_env(&mut args);

        // The env var must not clobber a key the user named explicitly.
        assert_eq!(args.vt_api_key, vec!["cli-vt"]);
        assert!(direct.vt);
    }

    #[test]
    fn test_seed_api_keys_reports_no_direct_source_when_environment_is_empty() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&[
            "URX_VT_API_KEY",
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
            "URX_BEVIGIL_API_KEY",
        ]);

        let mut args = Args::parse_from(["urx", "example.com"]);
        let direct = seed_api_keys_from_env(&mut args);

        assert!(
            !direct.vt && !direct.urlscan && !direct.zoomeye && !direct.github && !direct.bevigil
        );
    }
}
