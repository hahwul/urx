use std::process::Command;

#[test]
fn cli_rejects_unknown_output_formats_as_usage_errors() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = Command::new(env!("CARGO_BIN_EXE_urx"))
        .args(["--completions", "zsh", "-f", "xml"])
        .env("HOME", home.path())
        .output()
        .expect("urx should run");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(
        stderr.contains("invalid value 'xml'"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("possible values: plain, json, jsonl, csv, wordlist"),
        "valid formats should be listed: {stderr}"
    );
}

#[test]
fn invalid_output_format_in_config_still_warns() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let config_path = home.path().join("config.toml");
    std::fs::write(&config_path, "[output]\nformat = \"yaml\"\n")
        .expect("temporary config should be written");

    // `cache stats` applies the config and exits without contacting providers.
    // Its default cache path also lives under the temporary HOME and is only
    // checked for existence by this read-only subcommand.
    let output = Command::new(env!("CARGO_BIN_EXE_urx"))
        .arg("--config")
        .arg(&config_path)
        .args(["cache", "stats"])
        .env("HOME", home.path())
        .output()
        .expect("urx should run");

    assert!(
        output.status.success(),
        "cache stats should finish: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(
        stderr.contains(
            "Ignoring [output].format=\"yaml\" in config: expected plain, json, jsonl, csv, or wordlist"
        ),
        "invalid configured formats should still warn: {stderr}"
    );
}
