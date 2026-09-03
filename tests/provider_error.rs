use std::process::Command;

#[test]
fn no_valid_providers_error_lists_every_provider() {
    let config_dir = tempfile::tempdir().expect("temporary config directory should be created");
    let config_path = config_dir.path().join("config.toml");
    let provider_config_path = config_dir.path().join("provider-config.toml");
    std::fs::write(&config_path, "").expect("empty config should be written");
    std::fs::write(&provider_config_path, "").expect("empty provider config should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_urx"))
        .args([
            "example.com",
            "--providers",
            "wayback",
            "--exclude-providers",
            "wayback,robots,sitemap",
        ])
        .arg("--config")
        .arg(config_path)
        .arg("--provider-config")
        .arg(provider_config_path)
        .env_remove("URX_VT_API_KEY")
        .env_remove("URX_URLSCAN_API_KEY")
        .env_remove("URX_ZOOMEYE_API_KEY")
        .env_remove("URX_GITHUB_API_KEY")
        .env_remove("URX_BEVIGIL_API_KEY")
        .output()
        .expect("urx should run");

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(
        stderr.contains(
            "valid provider names (arquivo, bevigil, cc, github, otx, robots, sitemap, urlscan, vt, wayback, zoomeye)",
        ),
        "unexpected stderr: {stderr}"
    );
}
