//! Compiler versions, such as the cl.exe version `19.52.37029`.

use std::fmt;

/// A three-part version, compared part by part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// The first part (`19` in `19.52.37029`).
    pub major: u32,
    /// The second part.
    pub minor: u32,
    /// The third part.
    pub patch: u32,
}

impl Version {
    /// The first `<n>.<n>.<n>` in `text`, if any. Used on compiler banners,
    /// where the digits are the same in every display language even when the
    /// words around them are not.
    pub fn find_in(text: &str) -> Option<Version> {
        let bytes = text.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            if bytes[start].is_ascii_digit()
                && (start == 0 || !bytes[start - 1].is_ascii_digit())
                && let Some(version) = parse_at(&text[start..])
            {
                return Some(version);
            }
            start += 1;
        }
        None
    }
}

/// `<n>.<n>.<n>` at the start of `text`.
fn parse_at(text: &str) -> Option<Version> {
    let mut parts = [0u32; 3];
    let mut rest = text;
    for (i, part) in parts.iter_mut().enumerate() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        *part = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        if i < 2 {
            rest = rest.strip_prefix('.')?;
        }
    }
    Some(Version {
        major: parts[0],
        minor: parts[1],
        patch: parts[2],
    })
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// Mutation: return the last match, or accept two parts; this test fails.
    #[test]
    fn finds_the_first_three_part_version() {
        assert_eq!(
            Version::find_in("Microsoft (R) C/C++ Optimizing Compiler Version 19.52.37029 for x64"),
            Some(v(19, 52, 37029))
        );
        assert_eq!(
            Version::find_in("x 1.2 then 3.4.5 and 6.7.8"),
            Some(v(3, 4, 5))
        );
        assert_eq!(Version::find_in("no version here 1.2"), None);
    }

    /// Mutation: drop the check that a version starts at a digit boundary;
    /// this test fails (the tail of a number too large to be a version part,
    /// `9.2.3` in `99999999999.2.3`, would be read).
    #[test]
    fn a_version_starts_where_a_number_starts() {
        assert_eq!(Version::find_in("x119.2.3"), Some(v(119, 2, 3)));
        assert_eq!(Version::find_in("1.2.3.4"), Some(v(1, 2, 3)));
        // Too large to be a version part: not read from its middle instead.
        assert_eq!(Version::find_in("99999999999.2.3"), None);
    }

    /// Mutation: compare as strings, or reorder the fields (the derived
    /// ordering compares them in declaration order); this test fails.
    #[test]
    fn versions_compare_numerically_major_first() {
        assert!(v(19, 9, 0) < v(19, 10, 0));
        assert!(v(20, 0, 0) > v(19, 99, 99));
        assert!(v(19, 5, 0) > v(19, 4, 99));
        assert_eq!(v(19, 52, 37029).to_string(), "19.52.37029");
    }
}
