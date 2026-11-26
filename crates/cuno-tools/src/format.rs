//! clang-format's configuration from `[fmt]`
//! (`docs/en/specs/dev-tools.md`, "`[fmt]`").

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cuno_core::{CunoError, Env, ExitClass};
use cuno_manifest::{Fmt, FmtStyle, Lang};
use cuno_toolchain::Runner;

/// clang-format, and what it is run with.
pub struct ClangFormat<'a> {
    /// The executable selection chose.
    pub program: &'a Path,
    /// The `.clang-format` to format with, whatever is at the root.
    pub style: &'a Path,
    /// The environment it runs in.
    pub env: &'a Env,
    /// How many files at once.
    pub jobs: usize,
}

/// A file clang-format could not format, or cuno could not read or write.
#[derive(Debug)]
pub struct FormatError {
    /// The file.
    pub file: PathBuf,
    /// What went wrong: clang-format's own message, or the I/O error.
    pub reason: String,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not format {}: {}",
            self.file.display(),
            self.reason
        )
    }
}

impl std::error::Error for FormatError {}

impl CunoError for FormatError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Failure
    }
}

/// clang-format refused the configuration written from `[fmt]`: a
/// settings problem, not one in the code.
#[derive(Debug)]
pub struct StyleRefused {
    /// What clang-format said, with the scratch file's path left out.
    pub reason: String,
}

impl fmt::Display for StyleRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "clang-format refuses the configuration from [fmt] in cuno.toml: {}",
            self.reason
        )
    }
}

impl std::error::Error for StyleRefused {}

impl CunoError for StyleRefused {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        Some("correct [fmt.clang-format] in cuno.toml".to_owned())
    }
}

/// Whether clang-format reads `tool`'s configuration, before any file is
/// formatted with it, so a key it does not know is reported as the
/// settings' problem rather than as the first file's.
pub fn check_style(tool: &ClangFormat<'_>, runner: &dyn Runner) -> Result<(), StyleRefused> {
    let style = format!("--style=file:{}", tool.style.display());
    let args: [&OsStr; 2] = ["--dump-config".as_ref(), style.as_ref()];
    let output = runner
        .run(tool.program, &args, tool.env)
        .map_err(|e| StyleRefused {
            reason: e.to_string(),
        })?;
    if output.success {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let shown = tool.style.display().to_string();
    let reason = said
        .lines()
        .find(|line| line.contains("error:"))
        .unwrap_or(said.trim())
        .replace(&format!("{shown}:"), "line ")
        .replace(&shown, "the configuration");
    Err(StyleRefused { reason })
}

/// What formatting found: the files whose formatting differs (rewritten
/// unless `check`), and those that could not be formatted, each in the
/// order given.
#[derive(Debug, Default)]
pub struct Formatted {
    /// Differed, and were rewritten unless `check`.
    pub changed: Vec<PathBuf>,
    /// Could not be formatted; left as they were.
    pub failed: Vec<FormatError>,
}

/// Formats `files` with `tool`: each is run through clang-format and
/// compared with what it holds; one that differs is rewritten unless
/// `check` (`docs/en/specs/commands.md`, "`cuno fmt`"). A file is never
/// rewritten with empty output, nor when it changed while clang-format
/// ran, and is replaced whole, never truncated in place.
pub fn format(
    files: &[PathBuf],
    tool: &ClangFormat<'_>,
    check: bool,
    runner: &(dyn Runner + Sync),
) -> Formatted {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result<bool, FormatError>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..tool.jobs.clamp(1, files.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(file) = files.get(i) else { break };
                    let result = format_one(file, tool, check, runner);
                    results.lock().unwrap().push((i, result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    let mut formatted = Formatted::default();
    for (i, result) in results {
        match result {
            Ok(true) => formatted.changed.push(files[i].clone()),
            Ok(false) => {}
            Err(e) => formatted.failed.push(e),
        }
    }
    formatted
}

/// Whether `file` differs from its formatting, writing it unless `check`.
fn format_one(
    file: &Path,
    tool: &ClangFormat<'_>,
    check: bool,
    runner: &(dyn Runner + Sync),
) -> Result<bool, FormatError> {
    let failed = |reason: String| FormatError {
        file: file.to_owned(),
        reason,
    };
    let held = std::fs::read(file).map_err(|e| failed(e.to_string()))?;
    let style = format!("--style=file:{}", tool.style.display());
    let args: [&OsStr; 2] = [style.as_ref(), file.as_os_str()];
    let output = runner
        .run(tool.program, &args, tool.env)
        .map_err(|e| failed(e.to_string()))?;
    if !output.success {
        let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(failed(if said.is_empty() {
            "clang-format failed and said nothing".to_owned()
        } else {
            said
        }));
    }
    if output.stdout == held {
        return Ok(false);
    }
    // Formatting never empties a file; a clang-format that printed nothing
    // failed without saying so, and its output would erase the source.
    if output.stdout.is_empty() {
        return Err(failed("clang-format printed nothing for it".to_owned()));
    }
    if !check {
        // Asked again as it is written: fmt never writes through a link.
        crate::write::code(file, &|_| true).map_err(|refusal| failed(refusal.to_string()))?;
        replace(file, &held, &output.stdout).map_err(failed)?;
    }
    Ok(true)
}

/// Replaces `file`, which held `held` when it was read, with `content`:
/// written beside it and renamed over it, so the file is never left
/// half-written; not at all when it has changed since, so an edit made
/// meanwhile is not lost.
fn replace(file: &Path, held: &[u8], content: &[u8]) -> Result<(), String> {
    let mut beside = file.as_os_str().to_owned();
    beside.push(format!(".cuno-fmt-{}", std::process::id()));
    let beside = PathBuf::from(beside);
    std::fs::write(&beside, content).map_err(|e| e.to_string())?;
    let now = std::fs::read(file).map_err(|e| e.to_string());
    let result = match now {
        Ok(now) if now != held => {
            Err("it changed while it was being formatted; run cuno fmt again".to_owned())
        }
        Ok(_) => std::fs::rename(&beside, file).map_err(|e| e.to_string()),
        Err(e) => Err(e),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&beside);
    }
    result
}

/// The file editors read, at the project root.
pub const FILE: &str = ".clang-format";

/// The first line of every `.clang-format` cuno writes: a file that does
/// not start with it is the user's, and is not overwritten
/// (`docs/en/specs/dev-tools.md`, "Configuration").
pub const MARKER: &str =
    "# Generated by cuno from [fmt] in cuno.toml, and overwritten: edit cuno.toml instead.";

/// `.clang-format` for `fmt` in a package of `lang`. One section with no
/// `Language` key, so every file takes it: every file is styled in the
/// package's language (`dev-tools.md`, "Target files"). A key written in
/// `[fmt.clang-format]` replaces cuno's own, since clang-format refuses a
/// key given twice.
pub fn clang_format_config(fmt: &Fmt, lang: Lang) -> String {
    let mut keys: Vec<(&str, String)> = Vec::new();
    match fmt.style {
        FmtStyle::Cuno => {
            keys.push(("BasedOnStyle", "LLVM".to_owned()));
            keys.push(("IndentWidth", "4".to_owned()));
            keys.push(("ColumnLimit", "100".to_owned()));
            keys.push(("BreakBeforeBraces", "Attach".to_owned()));
            let pointers = match lang {
                Lang::C => "Right",
                Lang::Cxx => "Left",
            };
            keys.push(("PointerAlignment", pointers.to_owned()));
            keys.push(("DerivePointerAlignment", "false".to_owned()));
            keys.push(("SortIncludes", "CaseSensitive".to_owned()));
            // A blank line keeps headers whose order matters apart.
            keys.push(("IncludeBlocks", "Preserve".to_owned()));
        }
        other => keys.push(("BasedOnStyle", based_on(other).to_owned())),
    }
    if let Some(width) = fmt.indent_width {
        set(&mut keys, "IndentWidth", width.to_string());
    }
    if let Some(limit) = fmt.column_limit {
        set(&mut keys, "ColumnLimit", limit.to_string());
    }
    if let Some(tabs) = fmt.use_tabs {
        set(
            &mut keys,
            "UseTab",
            if tabs { "ForIndentation" } else { "Never" }.to_owned(),
        );
        // A tab as wide as one level, or levels mix tabs and spaces: the
        // indent written last, `[fmt.clang-format]`'s, wins.
        if tabs {
            let width = match fmt.clang_format.get("IndentWidth") {
                Some(toml::Value::Integer(width)) => width.to_string(),
                _ => fmt
                    .indent_width
                    .unwrap_or_else(|| style_indent(fmt.style))
                    .to_string(),
            };
            set(&mut keys, "TabWidth", width);
        }
    }
    keys.retain(|(k, _)| !fmt.clang_format.contains_key(*k));
    let mut text = format!("{MARKER}\n");
    for (key, value) in keys {
        text.push_str(&format!("{key}: {value}\n"));
    }
    for (key, value) in &fmt.clang_format {
        text.push_str(&format!("{key}: {}\n", flow(value)));
    }
    text
}

/// Sets `key` to `value`, in place of any earlier value.
fn set(keys: &mut Vec<(&str, String)>, key: &'static str, value: String) {
    keys.retain(|(k, _)| *k != key);
    keys.push((key, value));
}

/// A base style's own indent width, which a tab has to match.
fn style_indent(style: FmtStyle) -> u32 {
    match style {
        FmtStyle::Cuno | FmtStyle::Microsoft | FmtStyle::Webkit => 4,
        FmtStyle::Llvm
        | FmtStyle::Google
        | FmtStyle::Mozilla
        | FmtStyle::Chromium
        | FmtStyle::Gnu => 2,
    }
}

/// clang-format's name for a base style.
fn based_on(style: FmtStyle) -> &'static str {
    // A match, so a new style has to say what clang-format calls it.
    match style {
        FmtStyle::Cuno | FmtStyle::Llvm => "LLVM",
        FmtStyle::Google => "Google",
        FmtStyle::Microsoft => "Microsoft",
        FmtStyle::Mozilla => "Mozilla",
        FmtStyle::Webkit => "WebKit",
        FmtStyle::Chromium => "Chromium",
        FmtStyle::Gnu => "GNU",
    }
}

/// A TOML value as YAML flow syntax, which clang-format reads.
fn flow(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => format!("'{d}'"),
        toml::Value::Array(items) => {
            let items: Vec<String> = items.iter().map(flow).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(table) => {
            let entries: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{k}: {}", flow(v)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(extra: &str) -> Fmt {
        let toml = format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{extra}");
        let root = std::path::Path::new("/p");
        cuno_manifest::parse_manifest(&toml, &root.join("cuno.toml"), root)
            .unwrap()
            .value
            .fmt
    }

    /// A clang-format that answers each file as `answer` says: its output,
    /// or `None` to fail; `during` runs while it "formats".
    struct Fake<'a> {
        answer: &'a (dyn Fn(&Path) -> Option<Vec<u8>> + Sync),
        during: &'a (dyn Fn(&Path) + Sync),
    }

    impl Runner for Fake<'_> {
        fn run(
            &self,
            _program: &Path,
            args: &[&OsStr],
            _env: &Env,
        ) -> std::io::Result<cuno_toolchain::Output> {
            let file = Path::new(args[1]);
            (self.during)(file);
            Ok(match (self.answer)(file) {
                Some(stdout) => cuno_toolchain::Output {
                    success: true,
                    stdout,
                    stderr: Vec::new(),
                },
                None => cuno_toolchain::Output {
                    success: false,
                    stdout: Vec::new(),
                    stderr: b"boom".to_vec(),
                },
            })
        }
    }

    fn run_fake(files: &[PathBuf], fake: &Fake<'_>, check: bool) -> Formatted {
        let env = Env::from_pairs([], cuno_core::NameCase::Insensitive);
        let tool = ClangFormat {
            program: Path::new("clang-format"),
            style: Path::new("style"),
            env: &env,
            jobs: 4,
        };
        format(files, &tool, check, fake)
    }

    /// Every file that fails is reported and left as it was; the others
    /// are still formatted; an unchanged file is not counted, and `check`
    /// writes nothing.
    ///
    /// Mutation: stop at the first failure, count an unchanged file, or
    /// write under `check`; this test fails.
    #[test]
    fn failures_are_collected_and_the_rest_formatted() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = ["a.c", "bad1.c", "b.c", "bad2.c", "same.c"]
            .iter()
            .map(|n| dir.path().join(n))
            .collect();
        for f in &files {
            std::fs::write(f, "x\n").unwrap();
        }
        let answer = |f: &Path| {
            let name = f.file_name().unwrap().to_str().unwrap();
            match name {
                n if n.starts_with("bad") => None,
                "same.c" => Some(b"x\n".to_vec()),
                _ => Some(b"y\n".to_vec()),
            }
        };
        let nothing = |_: &Path| {};
        let fake = Fake {
            answer: &answer,
            during: &nothing,
        };
        let checked = run_fake(&files, &fake, true);
        assert_eq!(checked.changed, [files[0].clone(), files[2].clone()]);
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "x\n");
        let done = run_fake(&files, &fake, false);
        assert_eq!(done.changed, [files[0].clone(), files[2].clone()]);
        assert_eq!(
            done.failed
                .iter()
                .map(|e| e.file.clone())
                .collect::<Vec<_>>(),
            [files[1].clone(), files[3].clone()]
        );
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "y\n");
        assert_eq!(std::fs::read_to_string(&files[1]).unwrap(), "x\n");
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(left.len(), files.len(), "no file left beside");
    }

    /// A source is not rewritten with empty output, nor when it changed
    /// while clang-format ran: either would lose the user's code.
    ///
    /// Mutation: write empty output, or write without reading the file
    /// again; this test fails.
    #[test]
    fn a_source_is_never_lost() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.c");
        std::fs::write(&file, "int  x;\n").unwrap();
        let empty = |_: &Path| Some(Vec::new());
        let nothing = |_: &Path| {};
        let done = run_fake(
            std::slice::from_ref(&file),
            &Fake {
                answer: &empty,
                during: &nothing,
            },
            false,
        );
        assert_eq!(done.failed.len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "int  x;\n");

        let tidy = |_: &Path| Some(b"int x;\n".to_vec());
        let edit = |f: &Path| std::fs::write(f, "int  x; // mine\n").unwrap();
        let done = run_fake(
            std::slice::from_ref(&file),
            &Fake {
                answer: &tidy,
                during: &edit,
            },
            false,
        );
        assert!(
            done.failed[0].reason.contains("changed while"),
            "{:?}",
            done.failed
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "int  x; // mine\n");
    }

    fn lines(text: &str) -> Vec<&str> {
        text.lines().filter(|l| !l.starts_with('#')).collect()
    }

    /// The cuno style is LLVM with the spec's indent, width, braces and
    /// include sorting, and each language's pointer side
    /// (`dev-tools.md`, the section on the cuno default style).
    ///
    /// Mutation: drop or change any of these keys, or give C the C++
    /// pointer side; this test fails.
    #[test]
    fn the_cuno_style_is_the_spec_s() {
        let cxx = clang_format_config(&fmt(""), Lang::Cxx);
        assert_eq!(
            lines(&cxx),
            [
                "BasedOnStyle: LLVM",
                "IndentWidth: 4",
                "ColumnLimit: 100",
                "BreakBeforeBraces: Attach",
                "PointerAlignment: Left",
                "DerivePointerAlignment: false",
                "SortIncludes: CaseSensitive",
                "IncludeBlocks: Preserve",
            ]
        );
        let c = clang_format_config(&fmt(""), Lang::C);
        assert!(lines(&c).contains(&"PointerAlignment: Right"), "{c}");
        assert!(cxx.starts_with("# Generated by cuno"), "{cxx}");
    }

    /// Every other style is clang-format's own, by its name.
    ///
    /// Mutation: map a style to another name; this test fails.
    #[test]
    fn a_named_style_is_clang_format_s() {
        for (style, name) in [
            ("llvm", "LLVM"),
            ("google", "Google"),
            ("microsoft", "Microsoft"),
            ("mozilla", "Mozilla"),
            ("webkit", "WebKit"),
            ("chromium", "Chromium"),
            ("gnu", "GNU"),
        ] {
            let text =
                clang_format_config(&fmt(&format!("[fmt]\nstyle = \"{style}\"\n")), Lang::Cxx);
            assert_eq!(
                lines(&text),
                [format!("BasedOnStyle: {name}").as_str()],
                "{style}"
            );
        }
    }

    /// `indent-width`, `column-limit` and `use-tabs` replace the style's
    /// values only when written; tabs are as wide as the indent.
    ///
    /// Mutation: drop a key, write it twice, or leave `TabWidth` out; this
    /// test fails.
    #[test]
    fn written_keys_replace_the_style_s() {
        let text = clang_format_config(
            &fmt("[fmt]\nindent-width = 3\ncolumn-limit = 90\nuse-tabs = true\n"),
            Lang::Cxx,
        );
        let l = lines(&text);
        for key in [
            "IndentWidth: 3",
            "ColumnLimit: 90",
            "UseTab: ForIndentation",
            "TabWidth: 3",
        ] {
            assert_eq!(l.iter().filter(|x| **x == key).count(), 1, "{key}: {text}");
        }
        assert!(!text.contains("IndentWidth: 4"), "{text}");
        let spaces = clang_format_config(
            &fmt("[fmt]\nstyle = \"google\"\nuse-tabs = false\n"),
            Lang::Cxx,
        );
        assert_eq!(lines(&spaces), ["BasedOnStyle: Google", "UseTab: Never"]);
    }

    /// With tabs on, a tab is one level wide whatever sets the level: a
    /// named style's own indent, or `[fmt.clang-format]`'s.
    ///
    /// Mutation: leave `TabWidth` out without `indent-width`, or take it
    /// from cuno's indent when `[fmt.clang-format]` sets another; this test
    /// fails.
    #[test]
    fn a_tab_is_one_level_wide() {
        for (style, width) in [("webkit", "4"), ("google", "2"), ("microsoft", "4")] {
            let text = clang_format_config(
                &fmt(&format!("[fmt]\nstyle = \"{style}\"\nuse-tabs = true\n")),
                Lang::Cxx,
            );
            assert!(
                lines(&text).contains(&format!("TabWidth: {width}").as_str()),
                "{text}"
            );
        }
        let text = clang_format_config(
            &fmt("[fmt]\nuse-tabs = true\n[fmt.clang-format]\nIndentWidth = 2\n"),
            Lang::Cxx,
        );
        assert!(lines(&text).contains(&"TabWidth: 2"), "{text}");
    }

    /// `[fmt.clang-format]` is passed as written, in YAML flow syntax, and
    /// replaces a key cuno would have written, which clang-format would
    /// refuse twice.
    ///
    /// Mutation: keep cuno's key beside the user's, or quote a value
    /// wrongly; this test fails.
    #[test]
    fn the_escape_hatch_replaces_cuno_s_keys() {
        let text = clang_format_config(
            &fmt(
                "[fmt.clang-format]\nPointerAlignment = \"Right\"\nIndentCaseLabels = true\n\
                 BraceWrapping = { AfterFunction = true }\nMacros = [\"A=x\", \"it's\"]\n",
            ),
            Lang::Cxx,
        );
        let l = lines(&text);
        assert_eq!(
            l.iter()
                .filter(|x| x.starts_with("PointerAlignment"))
                .count(),
            1
        );
        assert!(l.contains(&"PointerAlignment: 'Right'"), "{text}");
        assert!(l.contains(&"IndentCaseLabels: true"), "{text}");
        assert!(
            l.contains(&"BraceWrapping: {AfterFunction: true}"),
            "{text}"
        );
        assert!(l.contains(&"Macros: ['A=x', 'it''s']"), "{text}");
    }
}
