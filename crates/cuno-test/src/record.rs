//! Reading the result file a test executable writes: the `test` records of
//! `--list`, and what one `--exact` run reported. The format is documented
//! at the top of `headers/cuno/test_main.c`, which writes it; see
//! `docs/en/specs/testing.md`, "Contract between cuno and test
//! executables".

use std::fmt;

/// One registration `--list` reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    /// The test's name.
    pub name: String,
    /// The file it was registered from, as the compiler spelled it.
    pub file: String,
    /// Its line.
    pub line: u32,
    /// Whether it came through the macro API (`cuno/test.h`).
    pub from_macro: bool,
}

/// A failed check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The file, as the compiler spelled it.
    pub file: String,
    /// The line.
    pub line: u32,
    /// What failed, possibly over several lines.
    pub message: String,
}

/// How a test ended, as its result file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// It ran to its end (`done`).
    Done,
    /// It was skipped, for this reason (`skip`).
    Skipped(String),
    /// The file ends without either: the test did not finish (a crash, a
    /// call to `exit`), or wrote no file at all.
    Unfinished,
}

/// What one test's result file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Every failed check, in order.
    pub failures: Vec<Failure>,
    /// Every `info`, in order.
    pub infos: Vec<String>,
    /// How it ended.
    pub ending: Ending,
}

/// A line of a result file cuno cannot read: a bug in the runtime, or a
/// runtime from another version of cuno.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRecord(pub String);

impl fmt::Display for BadRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a result record cuno cannot read: `{}`", self.0)
    }
}

/// The fields of `line`, unescaped.
fn fields(line: &str) -> Vec<String> {
    line.split('\t').map(unescape).collect()
}

/// `\\`, `\t`, `\r` and `\n` back to what they stand for.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// The registrations of a `--list` result file.
pub fn registrations(text: &str) -> Result<Vec<Registration>, BadRecord> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let f = fields(line);
            match f.as_slice() {
                [kind, name, file, number, api] if kind == "test" => {
                    let line_number = number.parse().map_err(|_| BadRecord(line.to_owned()))?;
                    let from_macro = match api.as_str() {
                        "macro" => true,
                        "modern" => false,
                        _ => return Err(BadRecord(line.to_owned())),
                    };
                    Ok(Registration {
                        name: name.clone(),
                        file: file.clone(),
                        line: line_number,
                        from_macro,
                    })
                }
                _ => Err(BadRecord(line.to_owned())),
            }
        })
        .collect()
}

/// The report of an `--exact` result file; `None` for no file, which is
/// a test that did not finish.
pub fn report(text: Option<&str>) -> Result<Report, BadRecord> {
    let mut report = Report {
        failures: Vec::new(),
        infos: Vec::new(),
        ending: Ending::Unfinished,
    };
    for line in text.unwrap_or("").lines().filter(|line| !line.is_empty()) {
        let f = fields(line);
        match f.as_slice() {
            [kind, file, number, message] if kind == "fail" => report.failures.push(Failure {
                file: file.clone(),
                line: number.parse().map_err(|_| BadRecord(line.to_owned()))?,
                message: message.clone(),
            }),
            [kind, text] if kind == "info" => report.infos.push(text.clone()),
            [kind, reason] if kind == "skip" => report.ending = Ending::Skipped(reason.clone()),
            [kind] if kind == "done" => report.ending = Ending::Done,
            _ => return Err(BadRecord(line.to_owned())),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field of a `test` record is read, escapes undone, and the API
    /// told apart.
    ///
    /// Mutation: swap two fields, drop an escape, or read `macro` as
    /// `modern`; this test fails.
    #[test]
    fn registrations_are_read_field_by_field() {
        let text = "test\tadd works\tC:\\\\p\\\\src\\\\a.cpp\t3\tmodern\n\
                    test\ttab\\there\tb.c\t12\tmacro\n";
        assert_eq!(
            registrations(text).unwrap(),
            [
                Registration {
                    name: "add works".into(),
                    file: "C:\\p\\src\\a.cpp".into(),
                    line: 3,
                    from_macro: false,
                },
                Registration {
                    name: "tab\there".into(),
                    file: "b.c".into(),
                    line: 12,
                    from_macro: true,
                },
            ]
        );
    }

    /// A record cuno does not know is an error naming it, not something
    /// skipped.
    ///
    /// Mutation: skip unknown records, or accept a line number that is not
    /// one; this test fails.
    #[test]
    fn an_unknown_record_is_refused() {
        assert_eq!(
            registrations("test\ta\tb.c\tx\tmodern\n"),
            Err(BadRecord("test\ta\tb.c\tx\tmodern".into()))
        );
        assert!(registrations("test\ta\tb.c\t1\tother\n").is_err());
        assert!(report(Some("pass\n")).is_err());
    }

    /// Failures, infos and the ending are read in order; a file with no
    /// ending, or no file, is a test that did not finish.
    ///
    /// Mutation: take a missing ending as done, read `skip` as done, or
    /// drop an escape; this test fails.
    #[test]
    fn a_report_says_how_the_test_ended() {
        let r = report(Some(
            "info\tn = 1\nfail\tsrc/a.cpp\t4\teq failed\\n  left:  1\\n  right: 2\ndone\n",
        ))
        .unwrap();
        assert_eq!(r.infos, ["n = 1"]);
        assert_eq!(
            r.failures,
            [Failure {
                file: "src/a.cpp".into(),
                line: 4,
                message: "eq failed\n  left:  1\n  right: 2".into(),
            }]
        );
        assert_eq!(r.ending, Ending::Done);
        assert_eq!(
            report(Some("skip\tlater\n")).unwrap().ending,
            Ending::Skipped("later".into())
        );
        assert_eq!(
            report(Some("fail\ta.c\t1\tx\n")).unwrap().ending,
            Ending::Unfinished
        );
        assert_eq!(report(None).unwrap().ending, Ending::Unfinished);
    }
}
