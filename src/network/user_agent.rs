use rand::seq::SliceRandom;
use rand::Rng;

/// Centralized random User-Agent generator
///
/// - Produces realistic, modern browser User-Agents
/// - Covers desktop (Windows/macOS/Linux) and mobile (iOS/Android; phones/tablets)
/// - Randomizes version numbers and device models within plausible ranges
///
/// Usage:
/// - `random()` chooses between desktop and mobile with realistic weights
/// - `random_desktop()` forces a desktop UA
/// - `random_mobile()` forces a mobile UA
pub struct UserAgent;

impl UserAgent {
    /// Returns a random realistic User-Agent with desktop/mobile weighting.
    /// Roughly 65% desktop, 35% mobile.
    pub fn random() -> String {
        let mut rng = rand::thread_rng();
        let pick_mobile = rng.gen_bool(0.35);
        if pick_mobile {
            Self::random_mobile()
        } else {
            Self::random_desktop()
        }
    }

    /// Returns a random realistic desktop User-Agent.
    pub fn random_desktop() -> String {
        let mut rng = rand::thread_rng();
        let desktop_generators: &[fn(&mut rand::rngs::ThreadRng) -> String] = &[
            Self::ua_win_chrome,
            Self::ua_win_edge,
            Self::ua_win_firefox,
            Self::ua_macos_chrome,
            Self::ua_macos_safari,
            Self::ua_linux_chrome,
            Self::ua_linux_firefox,
        ];
        let f = desktop_generators
            .choose(&mut rng)
            .expect("desktop_generators not empty");
        f(&mut rng)
    }

    /// Returns a random realistic mobile User-Agent (phones and tablets).
    pub fn random_mobile() -> String {
        let mut rng = rand::thread_rng();
        let mobile_generators: &[fn(&mut rand::rngs::ThreadRng) -> String] = &[
            Self::ua_ios_iphone_safari,
            Self::ua_ios_ipad_safari,
            Self::ua_android_phone_chrome,
            Self::ua_android_tablet_chrome,
        ];
        let f = mobile_generators
            .choose(&mut rng)
            .expect("mobile_generators not empty");
        f(&mut rng)
    }

    // ----- Generators: Desktop -----

    fn ua_win_chrome(rng: &mut rand::rngs::ThreadRng) -> String {
        let win_nt = Self::pick(rng, &["10.0", "10.0", "10.0", "11.0"]); // Win11 still often reports 10.0; bias toward 10.0
        let (chrome, build, patch) = Self::chrome_ver(rng);
        format!("Mozilla/5.0 (Windows NT {win_nt}; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Safari/537.36")
    }

    fn ua_win_edge(rng: &mut rand::rngs::ThreadRng) -> String {
        let win_nt = Self::pick(rng, &["10.0", "10.0", "11.0"]);
        let (chrome, build, patch) = Self::chrome_ver(rng);
        // Edge uses Edg/ with usually same Chrome major; keep builds close
        let (edge_major, edge_build, edge_patch) = (chrome, build, patch);
        format!("Mozilla/5.0 (Windows NT {win_nt}; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Safari/537.36 Edg/{edge_major}.{edge_patch}.{edge_build}")
    }

    fn ua_win_firefox(rng: &mut rand::rngs::ThreadRng) -> String {
        let win_nt = Self::pick(rng, &["10.0", "10.0", "11.0"]);
        let ff = Self::firefox_major(rng);
        format!("Mozilla/5.0 (Windows NT {win_nt}; Win64; x64; rv:{ff}.0) Gecko/20100101 Firefox/{ff}.0")
    }

    fn ua_macos_chrome(rng: &mut rand::rngs::ThreadRng) -> String {
        let mac = Self::pick(
            rng,
            &[
                "10_15_7", "11_7_10", "12_7_6", "13_6_7", "14_6", "14_5", "14_4_1",
            ],
        );
        let (chrome, build, patch) = Self::chrome_ver(rng);
        format!("Mozilla/5.0 (Macintosh; Intel Mac OS X {mac}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Safari/537.36")
    }

    fn ua_macos_safari(rng: &mut rand::rngs::ThreadRng) -> String {
        let mac = Self::pick(rng, &["12_7_6", "13_6_7", "14_6", "14_5", "14_4_1"]);
        let safari_ver = Self::pick(rng, &["16.6", "17.0", "17.3", "17.4", "17.5", "17.6"]);
        // Safari WebKit build remains commonly 605.1.15 in UA
        format!("Mozilla/5.0 (Macintosh; Intel Mac OS X {mac}) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{safari_ver} Safari/605.1.15")
    }

    fn ua_linux_chrome(rng: &mut rand::rngs::ThreadRng) -> String {
        let (chrome, build, patch) = Self::chrome_ver(rng);
        format!("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Safari/537.36")
    }

    fn ua_linux_firefox(rng: &mut rand::rngs::ThreadRng) -> String {
        let ff = Self::firefox_major(rng);
        format!("Mozilla/5.0 (X11; Linux x86_64; rv:{ff}.0) Gecko/20100101 Firefox/{ff}.0")
    }

    // ----- Generators: Mobile -----

    fn ua_ios_iphone_safari(rng: &mut rand::rngs::ThreadRng) -> String {
        let ios = Self::pick(
            rng,
            &[
                "16_6", "17_0", "17_1", "17_2", "17_3", "17_4", "17_5", "17_6",
            ],
        );
        let version = ios.replace('_', ".");
        // Mobile build codes commonly seen in UA strings
        let mobile_build = Self::pick(rng, &["15E148", "16E227", "17E262", "20E247", "21E230"]);
        format!("Mozilla/5.0 (iPhone; CPU iPhone OS {ios} like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{version} Mobile/{mobile_build} Safari/604.1")
    }

    fn ua_ios_ipad_safari(rng: &mut rand::rngs::ThreadRng) -> String {
        let ios = Self::pick(
            rng,
            &["16_6", "17_0", "17_1", "17_3", "17_4", "17_5", "17_6"],
        );
        let version = ios.replace('_', ".");
        let mobile_build = Self::pick(rng, &["15E148", "16E227", "17E262", "20E247", "21E230"]);
        format!("Mozilla/5.0 (iPad; CPU OS {ios} like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{version} Mobile/{mobile_build} Safari/604.1")
    }

    fn ua_android_phone_chrome(rng: &mut rand::rngs::ThreadRng) -> String {
        let android = Self::pick(rng, &["10", "11", "12", "13", "14"]);
        let device = Self::pick(
            rng,
            &[
                "Pixel 5",
                "Pixel 6",
                "Pixel 6a",
                "Pixel 7",
                "Pixel 7 Pro",
                "Pixel 8",
                "SM-G991B", // Galaxy S21
                "SM-G996B", // Galaxy S21+
                "SM-G998B", // Galaxy S21 Ultra
                "SM-S911B", // Galaxy S23
                "SM-S916B", // Galaxy S23+
                "SM-S918B", // Galaxy S23 Ultra
                "CPH2409",  // OnePlus 10 Pro (regional)
                "VOG-L29",  // Huawei P30 Pro
            ],
        );
        let (chrome, build, patch) = Self::chrome_ver(rng);
        format!("Mozilla/5.0 (Linux; Android {android}; {device}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Mobile Safari/537.36")
    }

    fn ua_android_tablet_chrome(rng: &mut rand::rngs::ThreadRng) -> String {
        let android = Self::pick(rng, &["10", "11", "12", "13", "14"]);
        let device = Self::pick(
            rng,
            &[
                "SM-T870",  // Galaxy Tab S7
                "SM-X700",  // Galaxy Tab S8
                "SM-X706B", // Galaxy Tab S8+ 5G
                "Nexus 10",
                "Pixel Tablet",
            ],
        );
        let (chrome, build, patch) = Self::chrome_ver(rng);
        format!("Mozilla/5.0 (Linux; Android {android}; {device}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome}.{patch}.{build} Safari/537.36")
    }

    // ----- Helpers -----

    /// Picks a random element from slice.
    fn pick<T: Clone>(rng: &mut rand::rngs::ThreadRng, vals: &[T]) -> T {
        vals.choose(rng).expect("slice not empty").clone()
    }

    /// Generates a realistic Chrome version triplet:
    /// - major: 120..=128 (as of 2024/2025)
    /// - minor: always 0 in UA (Chrome/<major>.0.<build>.<patch>)
    /// - build: 6000..=7100
    /// - patch: 10..=200
    fn chrome_ver(rng: &mut rand::rngs::ThreadRng) -> (u32, u32, u32) {
        let major = rng.gen_range(120..=128);
        let build = rng.gen_range(6000..=7100);
        let patch = rng.gen_range(10..=200);
        (major, build, patch)
    }

    /// Generates a realistic Firefox major version: 115..=130
    fn firefox_major(rng: &mut rand::rngs::ThreadRng) -> u32 {
        rng.gen_range(115..=130)
    }
}

// Convenience free functions

/// Returns a random realistic User-Agent with desktop/mobile weighting.
/// Roughly 65% desktop, 35% mobile.
pub fn random_user_agent() -> String {
    UserAgent::random()
}

/// Returns a random realistic desktop User-Agent.
#[allow(dead_code)]
pub fn random_desktop_user_agent() -> String {
    UserAgent::random_desktop()
}

/// Returns a random realistic mobile User-Agent.
#[allow(dead_code)]
pub fn random_mobile_user_agent() -> String {
    UserAgent::random_mobile()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_any_user_agent() {
        let ua = random_user_agent();
        assert!(
            ua.starts_with("Mozilla/5.0"),
            "UA must start with Mozilla/5.0, got: {ua}"
        );
        assert!(ua.len() > 40, "UA too short: {ua}");
    }

    #[test]
    fn generates_desktop_user_agent() {
        let ua = random_desktop_user_agent();
        assert!(
            ua.contains("Windows NT") || ua.contains("Macintosh") || ua.contains("Linux"),
            "Desktop UA must mention Windows/macOS/Linux. UA: {ua}"
        );
    }

    #[test]
    fn generates_mobile_user_agent() {
        let ua = random_mobile_user_agent();
        assert!(
            ua.contains("Android") || ua.contains("iPhone") || ua.contains("iPad"),
            "Mobile UA must mention Android/iPhone/iPad. UA: {ua}"
        );
    }
}
