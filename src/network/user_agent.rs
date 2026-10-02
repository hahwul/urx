use std::hash::{BuildHasher, Hasher};

/// Realistic current browser User-Agents for `--random-agent`: desktop
/// (Windows/macOS/Linux) and mobile (iOS/Android).
const USER_AGENTS: [&str; 8] = [
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36",
];

/// Polite, tool-identifying User-Agent used when UA randomisation is off.
///
/// Sending *some* User-Agent is mandatory, not cosmetic: the Wayback CDX
/// endpoint (and some others) reject a request with no `User-Agent` header
/// outright with `400 Bad Request`. The `Mozilla/5.0 (compatible; …)` form is
/// the conventional "polite bot" shape that upstreams accept while still
/// honestly identifying urx.
pub fn default_user_agent() -> String {
    concat!(
        "Mozilla/5.0 (compatible; urx/",
        env!("CARGO_PKG_VERSION"),
        "; +https://github.com/hahwul/urx)"
    )
    .to_string()
}

/// One of [`USER_AGENTS`], picked at random. `RandomState` is seeded per
/// instance, which is all the randomness a UA pick needs.
pub fn random_user_agent() -> String {
    let n = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    USER_AGENTS[(n % USER_AGENTS.len() as u64) as usize].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_any_user_agent() {
        let ua = random_user_agent();
        assert!(USER_AGENTS.contains(&ua.as_str()));
        assert!(ua.starts_with("Mozilla/5.0"), "{ua}");
    }

    #[test]
    fn default_user_agent_is_nonempty_and_identifies_urx() {
        let ua = default_user_agent();
        // Must be non-empty: an absent UA makes the Wayback CDX API 400.
        assert!(!ua.is_empty());
        assert!(ua.contains("urx/"), "default UA should identify urx: {ua}");
        assert!(ua.contains(env!("CARGO_PKG_VERSION")));
    }
}
