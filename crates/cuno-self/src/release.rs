//! cuno's releases: where they are, what a version is, and the names of the
//! files each carries (`docs/en/specs/distribution.md`, "Installation" and
//! "Versioning policy").

use std::cmp::Ordering;
use std::fmt;

use crate::SelfError;
use crate::fetch::Fetch;

/// Where cuno's releases are published.
pub const RELEASES: &str = "https://github.com/itsakeyfut/cuno/releases";

/// A released version: `x.y.z` and nothing else, since no other form is
/// ever released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    /// `x`.
    pub major: u64,
    /// `y`.
    pub minor: u64,
    /// `z`.
    pub patch: u64,
}

impl Version {
    /// `text` as `x.y.z`, each part a decimal number; `None` for anything
    /// else (`v1.2.3`, `1.2`, `1.2.3-rc.1`).
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let mut next = || {
            parts
                .next()
                .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))?
                .parse()
                .ok()
        };
        let version = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        parts.next().is_none().then_some(version)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The zip a release of `version` carries. `.github/workflows/release.yml`
/// packs it under the same name.
pub fn zip_name(version: &str) -> String {
    format!("cuno-{version}-x86_64-pc-windows-msvc.zip")
}

/// Where the files of the release of `version` are downloaded from.
pub fn download_base(releases: &str, version: Version) -> String {
    format!("{releases}/download/v{version}")
}

/// The latest released version: `<releases>/latest` redirects to
/// `…/releases/tag/v<x.y.z>`.
pub fn latest(fetch: &dyn Fetch, releases: &str) -> Result<Version, SelfError> {
    let url = format!("{releases}/latest");
    let Some(location) = fetch.redirect(&url)? else {
        return Err(SelfError::NotRedirected { url });
    };
    let tag = location
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    tag.strip_prefix('v')
        .and_then(Version::parse)
        .ok_or_else(|| SelfError::Tag {
            tag: tag.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The name the release workflow packs is the one cuno downloads: a
    /// rename in one place alone is found here, not after a release ships.
    ///
    /// Mutation: change the target triple or the order of the parts in
    /// `zip_name`; this test fails.
    #[test]
    fn the_zip_name_is_the_one_release_yml_packs() {
        let workflow =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
        let yml = std::fs::read_to_string(&workflow).unwrap();
        let packed = zip_name("${{ needs.check-version.outputs.version }}");
        assert!(
            yml.contains(&format!("ZIP: {packed}\n")),
            "{packed} not in {}",
            workflow.display()
        );
    }

    /// Only `x.y.z`, ordered by number.
    ///
    /// Mutation: compare versions as strings, or accept a prefix or a
    /// prerelease; this test fails.
    #[test]
    fn versions_are_x_y_z_ordered_by_number() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.10.0") > v("0.9.0"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.1") > v("0.2.0"));
        assert_eq!(v("0.2.0"), v("0.2.0"));
        assert_eq!(v("1.20.3").to_string(), "1.20.3");
        for bad in [
            "v0.2.0",
            "0.2",
            "0.2.0.1",
            "0.2.0-rc.1",
            "0.2.x",
            "",
            "0..1",
            "+1.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    /// A fetch that answers one redirect.
    struct Redirect(Option<&'static str>);

    impl Fetch for Redirect {
        fn redirect(&self, url: &str) -> Result<Option<String>, SelfError> {
            assert_eq!(url, "https://r/latest");
            Ok(self.0.map(str::to_owned))
        }
        fn download(&self, _: &str, _: &Path) -> Result<(), SelfError> {
            unreachable!("the lookup downloads nothing")
        }
    }

    /// The version is the tag the latest-release address redirects to.
    ///
    /// Mutation: keep the `v`, read another segment, or accept no redirect;
    /// this test fails.
    #[test]
    fn the_latest_version_is_the_redirects_tag() {
        let latest = |to| latest(&Redirect(to), "https://r");
        assert_eq!(
            latest(Some("https://r/tag/v0.12.3")),
            Ok(Version {
                major: 0,
                minor: 12,
                patch: 3
            })
        );
        assert_eq!(
            latest(Some("https://r/tag/nightly")),
            Err(SelfError::Tag {
                tag: "nightly".to_owned()
            })
        );
        assert_eq!(
            latest(None),
            Err(SelfError::NotRedirected {
                url: "https://r/latest".to_owned()
            })
        );
    }
}
