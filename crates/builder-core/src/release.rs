// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Fetch the newest published CRISPAWN community ISO and reuse a verified local copy.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::image::Writer;

const RELEASES_URL: &str =
    "https://api.github.com/repos/bunbun-digital/crispawn/releases?per_page=30";
const MAX_ISO_BYTES: u64 = 100_000_000;

#[derive(Debug)]
pub struct CachedIso {
    pub path: PathBuf,
    pub tag: String,
    pub sha256: String,
    pub downloaded: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    published_at: Option<String>,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

fn choose_latest(mut releases: Vec<Release>) -> Result<(String, Asset), String> {
    // GitHub's /releases/latest omits prereleases, including the current alpha release.
    releases.retain(|release| !release.draft && release.published_at.is_some());
    releases.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    for release in releases {
        if let Some(asset) = release.assets.into_iter().find(|asset| {
            asset.name.starts_with("crispawn-community-")
                && asset.name.ends_with("-x86_64.iso")
                && asset
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                && asset.size > 0
                && asset.size < MAX_ISO_BYTES
        }) {
            return Ok((release.tag_name, asset));
        }
    }
    Err("No published CRISPAWN community ISO release was found on GitHub".into())
}

fn asset_url_is_official(url: &str, tag: &str, name: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path() == format!("/bunbun-digital/crispawn/releases/download/{tag}/{name}")
}

fn cache_dir() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
    Ok(PathBuf::from(root)
        .join("CRISPAWN")
        .join("Builder")
        .join("iso-cache"))
}

/// Check GitHub on each call. A matching asset is read from the persistent cache;
/// a changed release asset is downloaded once into the same directory.
pub fn latest_iso(mut progress: impl FnMut(u8)) -> Result<CachedIso, String> {
    progress(0);
    let client = Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?;
    let releases: Vec<Release> = client
        .get(RELEASES_URL)
        .header("User-Agent", "crispawn-builder")
        .header("Accept", "application/vnd.github+json")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not check CRISPAWN releases on GitHub: {error}"))?
        .json()
        .map_err(|error| format!("Invalid GitHub release response: {error}"))?;
    let (tag, asset) = choose_latest(releases)?;
    if !asset_url_is_official(&asset.browser_download_url, &tag, &asset.name) {
        return Err("GitHub release asset URL is unexpected".into());
    }
    let expected_digest = match asset.digest.as_deref() {
        Some(value) => Some(
            value
                .strip_prefix("sha256:")
                .filter(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or("GitHub release asset digest is invalid")?,
        ),
        None => None,
    };
    let dir = cache_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create ISO cache {}: {error}", dir.display()))?;
    let path = dir.join(format!("{}-{}", asset.id, asset.name));
    if fs::metadata(&path).is_ok_and(|metadata| metadata.is_file() && metadata.len() == asset.size)
    {
        let sha256 = Writer::base_digest(&path)
            .map_err(|error| format!("Could not read cached ISO {}: {error}", path.display()))?;
        if expected_digest.is_none_or(|expected| sha256.eq_ignore_ascii_case(expected)) {
            progress(100);
            return Ok(CachedIso {
                path,
                tag,
                sha256,
                downloaded: false,
            });
        }
    }
    let mut response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "crispawn-builder")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not download CRISPAWN ISO: {error}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(&dir).map_err(|error| {
        format!(
            "Could not create temporary ISO in {}: {error}",
            dir.display()
        )
    })?;
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("Could not read GitHub ISO response: {error}"))?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        if bytes > asset.size || bytes >= MAX_ISO_BYTES {
            return Err("Downloaded ISO exceeds the GitHub release asset size".into());
        }
        temporary
            .write_all(&buffer[..read])
            .map_err(|error| format!("Could not write downloaded ISO: {error}"))?;
        progress(((bytes * 100 / asset.size).min(100)) as u8);
    }
    if bytes != asset.size {
        return Err("Downloaded ISO is incomplete".into());
    }
    temporary
        .flush()
        .map_err(|error| format!("Could not flush downloaded ISO: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Could not sync downloaded ISO: {error}"))?;
    let sha256 = Writer::base_digest(temporary.path())
        .map_err(|error| format!("Could not hash downloaded ISO: {error}"))?;
    if expected_digest.is_some_and(|expected| !sha256.eq_ignore_ascii_case(expected)) {
        return Err("Downloaded ISO does not match GitHub's asset digest".into());
    }
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("Could not replace cached ISO: {error}"))?;
    }
    temporary
        .persist(&path)
        .map_err(|error| format!("Could not save downloaded ISO: {error}"))?;
    progress(100);
    Ok(CachedIso {
        path,
        tag,
        sha256,
        downloaded: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_asset_url_must_match_release() {
        let good = "https://github.com/bunbun-digital/crispawn/releases/download/v0.1/crispawn-community-v0.1-x86_64.iso";
        assert!(asset_url_is_official(
            good,
            "v0.1",
            "crispawn-community-v0.1-x86_64.iso"
        ));
        assert!(!asset_url_is_official(
            "https://example.com/x.iso",
            "v0.1",
            "crispawn-community-v0.1-x86_64.iso"
        ));
        assert!(!asset_url_is_official(
            good,
            "v0.2",
            "crispawn-community-v0.1-x86_64.iso"
        ));
    }

    #[test]
    fn prereleases_are_included() {
        let releases: Vec<Release> = serde_json::from_str(r#"[
            {"tag_name":"v0.1-alpha","published_at":"2026-11-23T00:00:00Z","draft":false,"assets":[
                {"id":1,"name":"crispawn-community-v0.1-alpha-x86_64.iso","size":100,"digest":null,
                 "browser_download_url":"https://github.com/bunbun-digital/crispawn/releases/download/v0.1-alpha/crispawn-community-v0.1-alpha-x86_64.iso"}]}
        ]"#).unwrap();
        assert_eq!(choose_latest(releases).unwrap().0, "v0.1-alpha");
    }
}
