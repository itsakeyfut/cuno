//! What reading the configuration reports: errors (exit code 3) and warnings,
//! each pointing at the place in the file it is about. See
//! `docs/en/specs/manifest.md`, "Unknown keys".

use std::fmt;
use std::path::{Path, PathBuf};

use cuno_core::{CunoError, ExitClass};

/// A place in a configuration file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The file, as shown to the user (see [`shown_path`]).
    pub file: PathBuf,
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1.
    pub column: usize,
}

impl Location {
    /// The line and column of byte `offset` in `text`, in `file`. An offset
    /// past the end, or inside a character, counts as where the text ends or
    /// that character starts, so this never panics.
    pub fn at(file: &Path, text: &str, offset: usize) -> Location {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &text[..offset];
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        Location {
            file: file.to_owned(),
            line: before.matches('\n').count() + 1,
            // A byte order mark is not a column the user can see.
            column: before[line_start..]
                .trim_start_matches('\u{feff}')
                .chars()
                .count()
                + 1,
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

/// `file` as shown in a message: relative to `cwd` when it is under it, as
/// the user would type it, else as it is.
pub fn shown_path(file: &Path, cwd: &Path) -> PathBuf {
    file.strip_prefix(cwd).unwrap_or(file).to_owned()
}

fn write_located(
    f: &mut fmt::Formatter<'_>,
    message: &str,
    location: &Option<Location>,
) -> fmt::Result {
    f.write_str(message)?;
    if let Some(location) = location {
        write!(f, "\n --> {location}")?;
    }
    Ok(())
}

/// Something in a configuration file cuno reads past.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigWarning {
    /// What is wrong.
    pub message: String,
    /// Where, when it is in a file.
    pub location: Option<Location>,
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_located(f, &self.message, &self.location)
    }
}

/// A configuration cuno cannot use: a setup problem, exit code 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    /// What is wrong.
    pub message: String,
    /// Where, when it is in a file.
    pub location: Option<Location>,
    /// How to fix it, when there is more to say than the message.
    pub help: Option<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_located(f, &self.message, &self.location)
    }
}

impl std::error::Error for ConfigError {}

impl CunoError for ConfigError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        self.help.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: count lines or columns from 0, count columns in bytes, count
    /// a byte order mark as a column, slice
    /// inside a character or past the end, or drop the location line; this
    /// test fails.
    #[test]
    fn messages_point_at_the_line_and_column() {
        let text = "[build]\nwanings = \"all\"\n  é = 1\n";
        let file = Path::new("Cuno.toml");
        assert_eq!(Location::at(file, text, 0).to_string(), "Cuno.toml:1:1");
        assert_eq!(Location::at(file, text, 8).to_string(), "Cuno.toml:2:1");
        let accented = text.find("= 1").unwrap();
        assert_eq!(
            Location::at(file, text, accented).to_string(),
            "Cuno.toml:3:5"
        );
        let inside = text.find('é').unwrap() + 1;
        assert_eq!(
            Location::at(file, text, inside).to_string(),
            "Cuno.toml:3:3"
        );
        assert_eq!(Location::at(file, text, 999).to_string(), "Cuno.toml:4:1");
        let bom = "\u{feff}zz = 1\n";
        assert_eq!(
            Location::at(file, bom, bom.find("zz").unwrap()).to_string(),
            "Cuno.toml:1:1"
        );

        let warning = ConfigWarning {
            message: "unknown key `build.wanings` in Cuno.toml, ignored".into(),
            location: Some(Location::at(file, text, 8)),
        };
        assert_eq!(
            warning.to_string(),
            "unknown key `build.wanings` in Cuno.toml, ignored\n --> Cuno.toml:2:1"
        );
        let error = ConfigError {
            message: "cannot read Cuno.toml".into(),
            location: None,
            help: Some("check it".into()),
        };
        assert_eq!(error.to_string(), "cannot read Cuno.toml");
        assert_eq!(error.help().as_deref(), Some("check it"));
    }

    /// Mutation: give a configuration error another exit class; this test
    /// fails.
    #[test]
    fn a_configuration_error_is_a_setup_problem() {
        let error = ConfigError {
            message: String::new(),
            location: None,
            help: None,
        };
        assert_eq!(error.exit_class(), ExitClass::Setup);
    }

    /// Mutation: always show the full path, or strip a directory the file is
    /// not under; this test fails.
    #[test]
    fn files_under_the_current_directory_are_shown_relative() {
        let cwd = Path::new("work").join("p");
        assert_eq!(
            shown_path(&cwd.join("Cuno.toml"), &cwd),
            Path::new("Cuno.toml")
        );
        let elsewhere = Path::new("home").join("config.toml");
        assert_eq!(shown_path(&elsewhere, &cwd), elsewhere);
    }
}
