//! Published stable releases only; no installation or access to the game library.
use serde::Deserialize;

pub const API_URL: &str = "https://api.github.com/repos/guizmox/PocketHLE/releases/latest";
const RELEASES: &str = "https://github.com/guizmox/PocketHLE/releases/";

#[derive(Clone, Copy, Debug)]
pub enum Platform {
    Windows,
    Linux,
    SteamDeck,
    Android,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn from_json(json: &str, installed: &str, platform: Platform) -> Option<Update> {
    let release: Release = serde_json::from_str(json).ok()?;
    let version = semver::Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .ok()?;
    let current = semver::Version::parse(installed).ok()?;
    if release.draft
        || release.prerelease
        || !version.pre.is_empty()
        || !version.cmp_precedence(&current).is_gt()
    {
        return None;
    }
    let assets: Vec<_> = release
        .assets
        .iter()
        .filter(|a| {
            let name = a.name.to_ascii_lowercase();
            let deck = name.contains("steamdeck") || name.contains("steam-deck");
            let suitable = match platform {
                Platform::Android => name.ends_with(".apk"),
                Platform::SteamDeck => deck && name.ends_with(".zip"),
                Platform::Windows => name.contains("windows") && !deck && name.ends_with(".zip"),
                Platform::Linux => {
                    name.contains("linux") && (name.ends_with(".tar.gz") || name.ends_with(".zip"))
                }
            };
            suitable
                && a.browser_download_url
                    .starts_with(&format!("{RELEASES}download/"))
        })
        .collect();
    // Ambiguous/missing platform packages still open the exact release page.
    let url = if assets.len() == 1 {
        assets[0].browser_download_url.clone()
    } else {
        format!("{RELEASES}tag/{}", release.tag_name)
    };
    Some(Update {
        version: version.to_string(),
        url,
    })
}

#[cfg(feature = "release-updates")]
pub fn check(installed: &str, platform: Platform) -> Option<Update> {
    use std::{io::Read, time::Duration};
    // Keep public roots on Proton even if its Windows certificate store is
    // empty, and also honor system trust/proxies on managed PCs.
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Ok(certificates) = rustls_native_certs::load_native_certs() {
        roots.add_parsable_certificates(certificates);
    }
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .try_proxy_from_env(true)
        .tls_config(std::sync::Arc::new(tls))
        .timeout(Duration::from_secs(8))
        .build();
    let response = agent
        .get(API_URL)
        .set(
            "User-Agent",
            concat!("PocketHLE/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    let mut json = String::new();
    response
        .into_reader()
        .take(1_048_577)
        .read_to_string(&mut json)
        .ok()?;
    if json.len() > 1_048_576 {
        return None;
    }
    from_json(&json, installed, platform)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_android_release_cases() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/releases.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let result = from_json(
                &case["release"].to_string(),
                case["installed"].as_str().unwrap(),
                Platform::Android,
            );
            assert_eq!(
                result.is_some(),
                case["available"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
            if let Some(update) = result {
                assert_eq!(
                    update.url,
                    case["url"].as_str().unwrap(),
                    "{}",
                    case["name"]
                );
            }
        }
    }
    fn release(tag: &str, assets: serde_json::Value) -> String {
        serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false,"assets":assets})
            .to_string()
    }
    #[test]
    fn semantic_order_and_unpublished_versions() {
        for tag in ["v0.3.1", "v0.2.9", "v0.4.0-beta.1", "nightly"] {
            assert!(from_json(
                &release(tag, serde_json::json!([])),
                "0.3.1",
                Platform::Windows
            )
            .is_none());
        }
        assert!(from_json(
            &release("v0.10.0", serde_json::json!([])),
            "0.9.0",
            Platform::Windows
        )
        .is_some());
        assert!(from_json(
            &release("v0.3.1", serde_json::json!([])),
            "0.3.1-android-test3",
            Platform::Android
        )
        .is_some());
        for field in ["draft", "prerelease"] {
            let mut value: serde_json::Value =
                serde_json::from_str(&release("v0.4.0", serde_json::json!([]))).unwrap();
            value[field] = true.into();
            assert!(from_json(&value.to_string(), "0.3.1", Platform::Windows).is_none());
        }
        assert!(from_json("{}", "0.3.1", Platform::Windows).is_none());
    }
    #[test]
    fn correct_platform_package_and_safe_fallback() {
        let assets = serde_json::json!([
            {"name":"PocketHLE-windows-x86_64.zip","browser_download_url":format!("{RELEASES}download/v0.4.0/windows.zip")},
            {"name":"PocketHLE-steamdeck-x86_64.zip","browser_download_url":format!("{RELEASES}download/v0.4.0/deck.zip")},
            {"name":"app-debug.apk","browser_download_url":format!("{RELEASES}download/v0.4.0/android.apk")}
        ]);
        let json = release("v0.4.0", assets);
        for (p, suffix) in [
            (Platform::Windows, "windows.zip"),
            (Platform::SteamDeck, "deck.zip"),
            (Platform::Android, "android.apk"),
        ] {
            assert!(from_json(&json, "0.3.1", p).unwrap().url.ends_with(suffix));
        }
        assert_eq!(
            from_json(&json, "0.3.1", Platform::Linux).unwrap().url,
            format!("{RELEASES}tag/v0.4.0")
        );
        let bad = release(
            "v0.4.0",
            serde_json::json!([{"name":"app.apk","browser_download_url":"https://example.org/app.apk"}]),
        );
        assert_eq!(
            from_json(&bad, "0.3.1", Platform::Android).unwrap().url,
            format!("{RELEASES}tag/v0.4.0")
        );
    }
    #[test]
    fn linux_zip_and_tar_packages_are_supported() {
        for name in [
            "PocketHLE-linux-x86_64.zip",
            "PocketHLE-linux-x86_64.tar.gz",
        ] {
            let url = format!("{RELEASES}download/v0.4.0/{name}");
            let json = release(
                "v0.4.0",
                serde_json::json!([{"name":name,"browser_download_url":url}]),
            );
            assert_eq!(from_json(&json, "0.3.1", Platform::Linux).unwrap().url, url);
        }
    }
}
