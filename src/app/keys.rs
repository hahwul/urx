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

/// The `--<id>-api-key` slot in `args` for keyed provider `id`.
pub fn api_keys_mut<'a>(args: &'a mut Args, id: &str) -> &'a mut Vec<String> {
    match id {
        "vt" => &mut args.vt_api_key,
        "urlscan" => &mut args.urlscan_api_key,
        "zoomeye" => &mut args.zoomeye_api_key,
        "github" => &mut args.github_api_key,
        "bevigil" => &mut args.bevigil_api_key,
        _ => unreachable!("{id} is not a keyed provider"),
    }
}

/// Every API key urx resolved for this run, aligned with [`KEYED_PROVIDER_IDS`].
#[derive(Debug, Default, Clone)]
pub struct ApiKeys([Vec<String>; 5]);

impl ApiKeys {
    /// Merge each provider's CLI/config keys with its environment variable.
    pub fn resolve(args: &Args) -> Self {
        // A scratch copy, so the one `api_keys_mut` table serves reads too.
        let mut args = args.clone();
        Self(KEYED_PROVIDER_IDS.map(|id| {
            parse_api_keys(
                std::mem::take(api_keys_mut(&mut args, id)),
                &api_key_env_var(id),
            )
        }))
    }

    /// The keys resolved for provider `id`, or an empty slice for a keyless one.
    pub fn for_provider(&self, id: &str) -> &[String] {
        KEYED_PROVIDER_IDS
            .iter()
            .position(|k| *k == id)
            .map_or(&[], |i| &self.0[i])
    }
}

/// Parse a comma-separated key list out of `env_var_name`.
///
/// Blank entries are dropped so a trailing comma or a `KEY=` with nothing after
/// it doesn't produce an empty key that later reads as an auth failure.
fn parse_env_api_keys(env_var_name: &str) -> Vec<String> {
    std::env::var(env_var_name)
        .map(|keys| crate::utils::split_csv(&keys))
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

/// Fill empty API-key args from their environment variables, and return the
/// providers that ended up with a user-supplied key.
///
/// This must run *before* any config file is applied, which is also what makes
/// the return value trustworthy: at this point a non-empty field can only have
/// come from the CLI or from the environment.
pub fn seed_api_keys_from_env(args: &mut Args) -> Vec<&'static str> {
    KEYED_PROVIDER_IDS
        .into_iter()
        .filter(|id| {
            let slot = api_keys_mut(args, id);
            if slot.is_empty() {
                *slot = parse_env_api_keys(&api_key_env_var(id));
            }
            !slot.is_empty()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliProvided;
    use crate::config::{self, Config};
    use crate::test_support::{EnvGuard, ENV};
    use clap::Parser;

    #[test]
    fn test_api_key_names_are_derived_from_the_provider_id() {
        assert_eq!(api_key_env_var("vt"), "URX_VT_API_KEY");
        assert_eq!(api_key_env_var("zoomeye"), "URX_ZOOMEYE_API_KEY");
        assert_eq!(api_key_flag("github"), "--github-api-key");
    }

    #[test]
    fn test_parse_api_keys() {
        // CLI keys only
        let cli_keys = vec!["key1".to_string(), "key2".to_string()];
        let result = parse_api_keys(cli_keys, "NONEXISTENT_ENV_VAR");
        assert_eq!(result, vec!["key1", "key2"]);

        let _env_lock = ENV.lock().unwrap();

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
        let _env_lock = ENV.lock().unwrap();
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
        assert_eq!(keys.for_provider("vt"), ["vt_key1", "vt_key2"]);
        assert_eq!(keys.for_provider("urlscan"), ["url_key1"]);
    }

    #[test]
    fn test_api_key_precedence() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env_vt_key")]);

        // An explicit CLI key sorts ahead of the environment's.
        let args = Args::parse_from(["urx", "example.com", "--vt-api-key", "arg_vt_key"]);
        let keys = ApiKeys::resolve(&args);
        assert_eq!(keys.for_provider("vt"), ["arg_vt_key", "env_vt_key"]);

        // Without one, the environment variable is the fallback.
        let args = Args::parse_from(["urx", "example.com"]);
        assert_eq!(ApiKeys::resolve(&args).for_provider("vt"), ["env_vt_key"]);
    }

    #[test]
    fn test_for_provider_maps_ids_and_ignores_keyless_ones() {
        let keys = ApiKeys(KEYED_PROVIDER_IDS.map(|id| vec![id.to_string()]));
        for id in KEYED_PROVIDER_IDS {
            assert_eq!(keys.for_provider(id), [id], "{id} should map to its keys");
        }
        assert!(keys.for_provider("wayback").is_empty());
    }

    #[test]
    fn test_env_api_keys_override_config_layers() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("URX_VT_API_KEY", "env-vt-1,env-vt-2"),
            ("URX_URLSCAN_API_KEY", "env-urlscan"),
            ("URX_ZOOMEYE_API_KEY", "env-zoomeye"),
        ]);
        let _github_guard = EnvGuard::unset(&["URX_GITHUB_API_KEY", "URX_BEVIGIL_API_KEY"]);

        let mut args = Args::parse_from(["urx", "example.com"]);
        let direct = seed_api_keys_from_env(&mut args);
        assert_eq!(direct, ["vt", "urlscan", "zoomeye"]);

        let mut config = Config::default();
        config.provider.vt_api_key = Some("config-vt".to_string());
        config.provider.urlscan_api_key = Some("config-urlscan".to_string());
        config.provider.zoomeye_api_key = Some("config-zoomeye".to_string());
        config.apply_to_args(&mut args, &CliProvided::default());

        let provider_keys = config::ProviderKeysConfig {
            vt_api_key: Some("provider-vt".to_string()),
            urlscan_api_key: Some("provider-urlscan".to_string()),
            zoomeye_api_key: Some("provider-zoomeye".to_string()),
            ..Default::default()
        };
        provider_keys.apply_to_args(
            &mut args,
            config::CliSuppliedKeys {
                api_keys: direct,
                notify: false,
            },
        );

        assert_eq!(args.vt_api_key, vec!["env-vt-1", "env-vt-2"]);
        assert_eq!(args.urlscan_api_key, vec!["env-urlscan"]);
        assert_eq!(args.zoomeye_api_key, vec!["env-zoomeye"]);
    }

    #[test]
    fn test_seed_api_keys_leaves_cli_values_alone() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env-vt")]);

        let mut args = Args::parse_from(["urx", "example.com", "--vt-api-key", "cli-vt"]);
        let direct = seed_api_keys_from_env(&mut args);

        // The env var must not clobber a key the user named explicitly.
        assert_eq!(args.vt_api_key, vec!["cli-vt"]);
        assert_eq!(direct, ["vt"]);
    }

    #[test]
    fn test_seed_api_keys_reports_no_direct_source_when_environment_is_empty() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::unset(&[
            "URX_VT_API_KEY",
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
            "URX_BEVIGIL_API_KEY",
        ]);

        let mut args = Args::parse_from(["urx", "example.com"]);
        assert!(seed_api_keys_from_env(&mut args).is_empty());
    }
}
