//! Writing what cuno shows the user: errors, warnings, help and status lines,
//! in color or not. See `docs/en/specs/commands.md`, "Output".

use std::fmt::Display;
use std::io::Write;

use crate::Env;

/// A color choice given with `--color` or `[term] color`
/// (`docs/en/specs/commands.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorChoice {
    /// Color only when writing to a terminal.
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

const RESET: &str = "\x1b[0m";
const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_YELLOW: &str = "\x1b[1;33m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_GREEN: &str = "\x1b[1;32m";

/// Whether to color: `--color` (`given`), then `[term] color`
/// (`configured`), then `NO_COLOR` (`no_color`, set to anything, even
/// empty), then whether the output is a `terminal`. The one place this
/// order is written; see `docs/en/specs/commands.md`, "Available for all
/// commands".
pub fn colors(
    given: Option<ColorChoice>,
    configured: Option<ColorChoice>,
    no_color: bool,
    terminal: bool,
) -> bool {
    match given.or(configured) {
        Some(ColorChoice::Always) => true,
        Some(ColorChoice::Never) => false,
        Some(ColorChoice::Auto) => terminal,
        None => terminal && !no_color,
    }
}

/// How much a command shows: `-q`, nothing given, or `-v` (or
/// `[term] verbose`, which `-q` still wins over). One value, so quiet and
/// verbose cannot both be in effect (`docs/en/specs/commands.md`,
/// "Available for all commands").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verbosity {
    /// `-q`: status lines are left out.
    Quiet,
    /// What every command shows by default.
    #[default]
    Normal,
    /// `-v` or `[term] verbose`: also each step's command.
    Verbose,
}

/// Where cuno's messages go (stderr for the binary). Crates write through a
/// `Shell` instead of printing, so the color decision is made once.
pub struct Shell {
    out: Box<dyn Write>,
    /// `--color`, when given.
    given: Option<ColorChoice>,
    /// Whether `NO_COLOR` is set, even to nothing.
    no_color: bool,
    /// Whether `out` is a terminal.
    terminal: bool,
    color: bool,
    verbosity: Verbosity,
}

impl Shell {
    /// A shell writing to `out`. `choice` is the color choice the user gave,
    /// or `None` when they gave none; `out_is_terminal` says whether `out` is
    /// a terminal.
    ///
    /// With no choice given, `NO_COLOR` (set to anything, including empty)
    /// means no color, and otherwise color follows the terminal. A choice the
    /// user gave always wins over `NO_COLOR`, including an explicit `auto`.
    /// [`Shell::configure`] adds `[term] color` once it is read. See
    /// `docs/en/specs/commands.md`, "Available for all commands".
    pub fn new(
        out: Box<dyn Write>,
        choice: Option<ColorChoice>,
        env: &Env,
        out_is_terminal: bool,
    ) -> Shell {
        let mut shell = Shell {
            out,
            given: choice,
            no_color: env.get("NO_COLOR").is_some(),
            terminal: out_is_terminal,
            color: false,
            verbosity: Verbosity::Normal,
        };
        shell.configure(None, false);
        shell
    }

    /// Applies `[term]`, which is read after the shell is made: `color`
    /// ranks below `--color` and above `NO_COLOR`, and `verbose` makes the
    /// shell verbose unless `-q` or `-v` already decided.
    pub fn configure(&mut self, configured: Option<ColorChoice>, verbose: bool) {
        self.color = colors(self.given, configured, self.no_color, self.terminal);
        if verbose && self.verbosity == Verbosity::Normal {
            self.verbosity = Verbosity::Verbose;
        }
    }

    /// Applies `-q` or `-v`, once, before anything is written. Quiet, a
    /// [`Shell::status`] line writes nothing, and errors, warnings, help and
    /// other programs' output are written as before
    /// (`docs/en/specs/commands.md`, "Available for all commands").
    pub fn set_verbosity(&mut self, verbosity: Verbosity) {
        self.verbosity = verbosity;
    }

    /// Whether to show what only `-v` shows, such as each step's command.
    pub fn verbose(&self) -> bool {
        self.verbosity == Verbosity::Verbose
    }

    /// Whether this shell writes color escape codes.
    pub fn uses_color(&self) -> bool {
        self.color
    }

    /// `error: <message>`.
    pub fn error(&mut self, message: impl Display) {
        self.line(BOLD_RED, "error", message);
    }

    /// `warning: <message>`.
    pub fn warning(&mut self, message: impl Display) {
        self.line(BOLD_YELLOW, "warning", message);
    }

    /// `help: <message>`, how to fix what the previous line reported.
    pub fn help(&mut self, message: impl Display) {
        self.line(BOLD_CYAN, "help", message);
    }

    /// A status line: `verb` right-aligned in 12 columns, then `message`, as
    /// in `   Compiling mygame v0.1.0`. Nothing under `-q`
    /// ([`Shell::set_verbosity`]).
    pub fn status(&mut self, verb: &str, message: impl Display) {
        if self.verbosity == Verbosity::Quiet {
            return;
        }
        let verb = self.paint(BOLD_GREEN, &format!("{verb:>12}"));
        self.write(format_args!("{verb} {message}"));
    }

    /// Another program's output (a compiler's diagnostics), written as it
    /// was produced: not decoded, so text in the console's code page reaches
    /// the console as such, and without anything added.
    pub fn raw(&mut self, bytes: &[u8]) {
        // A failed write to stderr cannot be reported anywhere better.
        let _ = self.out.write_all(bytes);
        let _ = self.out.flush();
    }

    fn line(&mut self, style: &str, label: &str, message: impl Display) {
        let label = self.paint(style, label);
        self.write(format_args!("{label}: {message}"));
    }

    fn write(&mut self, line: std::fmt::Arguments<'_>) {
        // A failed write to stderr cannot be reported anywhere better.
        let _ = writeln!(self.out, "{line}");
    }

    fn paint(&self, style: &str, text: &str) -> String {
        if self.color {
            format!("{style}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::NameCase;
    use std::sync::{Arc, Mutex};

    /// A writer the test can read back after the `Shell` has written to it.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn shell(
        choice: Option<ColorChoice>,
        env: &[(&str, &str)],
        terminal: bool,
    ) -> (Shell, Captured) {
        let out = Captured::default();
        let env = Env::from_pairs(
            env.iter().map(|(n, v)| (n.into(), v.into())),
            NameCase::Sensitive,
        );
        (
            Shell::new(Box::new(out.clone()), choice, &env, terminal),
            out,
        )
    }

    /// `[term] verbose` makes a shell verbose only when the command line
    /// said neither `-q` nor `-v`: `-q` wins over it, and it never makes a
    /// shell less verbose (`docs/en/specs/commands.md`, "Available for all
    /// commands").
    ///
    /// Mutation: start a new shell verbose, let `[term] verbose` override
    /// `-q`, ignore it, or let `false` quiet a `-v`; this test fails.
    #[test]
    fn configured_verbose_ranks_below_the_command_line() {
        // A new shell is neither quiet nor verbose until told.
        let (fresh, _) = shell(Some(ColorChoice::Never), &[], false);
        assert!(!fresh.verbose());
        for (given, configured, verbose, quiet) in [
            (Verbosity::Normal, true, true, false),
            (Verbosity::Normal, false, false, false),
            (Verbosity::Quiet, true, false, true),
            (Verbosity::Verbose, false, true, false),
        ] {
            let (mut sh, out) = shell(Some(ColorChoice::Never), &[], false);
            sh.set_verbosity(given);
            sh.configure(None, configured);
            assert_eq!(sh.verbose(), verbose, "{given:?}, configured {configured}");
            sh.status("Compiling", "app");
            assert_eq!(
                out.text().is_empty(),
                quiet,
                "{given:?}, configured {configured}"
            );
        }
    }

    /// Under `-q` a status line is left out, and everything else is still
    /// written.
    ///
    /// Mutation: ignore `quiet` in `status`, or return early from `error`,
    /// `warning`, `help` or `raw` too; this test fails.
    #[test]
    fn quiet_leaves_out_status_lines_only() {
        let (mut sh, out) = shell(Some(ColorChoice::Never), &[], false);
        sh.set_verbosity(Verbosity::Quiet);
        sh.status("Compiling", "mygame v0.1.0");
        sh.error("no compiler found");
        sh.help("install Build Tools");
        sh.warning("unused key");
        sh.raw(
            b"main.cpp(1): note
",
        );
        assert_eq!(
            out.text(),
            "error: no compiler found
help: install Build Tools
warning: unused key
main.cpp(1): note
"
        );
    }

    /// Mutation: change a label in `error`, `warning`, `help` or `status`,
    /// or the width in `status`; this test fails.
    #[test]
    fn lines_without_color() {
        let (mut sh, out) = shell(Some(ColorChoice::Never), &[], false);
        sh.error("no compiler found");
        sh.help("install Build Tools");
        sh.warning("unused key");
        sh.status("Compiling", "mygame v0.1.0");
        assert_eq!(
            out.text(),
            "error: no compiler found\nhelp: install Build Tools\nwarning: unused key\n   Compiling mygame v0.1.0\n"
        );
    }

    /// Another program's output goes through byte for byte, with nothing
    /// added and nothing painted, even when it is not UTF-8.
    ///
    /// Mutation: decode it, add a newline, or paint it; this test fails.
    #[test]
    fn raw_output_is_written_as_produced() {
        let (mut sh, out) = shell(Some(ColorChoice::Always), &[], false);
        let cp932 = [0x83, 0x81, 0x83, 0x82, 0x0d, 0x0a];
        sh.raw(&cp932);
        sh.raw(b"x");
        assert_eq!(*out.0.lock().unwrap(), [&cp932[..], b"x"].concat());
    }

    /// Mutation: swap or drop the style of any label, or stop painting the
    /// status verb; this test fails.
    #[test]
    fn lines_with_color_paint_only_the_label() {
        let (mut sh, out) = shell(Some(ColorChoice::Always), &[], false);
        sh.error("e");
        sh.help("h");
        sh.warning("w");
        sh.status("Compiling", "m");
        assert_eq!(
            out.text(),
            "\x1b[1;31merror\x1b[0m: e\n\
             \x1b[1;36mhelp\x1b[0m: h\n\
             \x1b[1;33mwarning\x1b[0m: w\n\
             \x1b[1;32m   Compiling\x1b[0m m\n"
        );
    }

    /// Mutation: let `NO_COLOR` override an explicit choice (including an
    /// explicit `auto`), ignore it when no choice was given, or color when not
    /// a terminal under `auto`; this test fails.
    #[test]
    fn color_follows_the_choice_the_terminal_and_no_color() {
        let on =
            |choice, env: &[(&str, &str)], terminal| shell(choice, env, terminal).0.uses_color();
        let no_color: &[(&str, &str)] = &[("NO_COLOR", "")];
        // No choice given: the terminal decides, and NO_COLOR (even empty)
        // turns color off.
        assert!(on(None, &[], true));
        assert!(!on(None, &[], false));
        assert!(!on(None, no_color, true));
        // A choice given wins over NO_COLOR.
        assert!(on(Some(ColorChoice::Auto), no_color, true));
        assert!(!on(Some(ColorChoice::Auto), &[], false));
        assert!(on(Some(ColorChoice::Always), no_color, false));
        assert!(!on(Some(ColorChoice::Never), &[], true));
    }

    /// `--color`, then `[term] color`, then `NO_COLOR`, then the terminal,
    /// each level deciding only when every level above it gave nothing.
    ///
    /// Mutation: swap any two levels, let `NO_COLOR` beat a choice given or
    /// configured, or let `auto` ignore the terminal; this test fails.
    #[test]
    fn colors_follows_the_order() {
        use ColorChoice::{Always, Auto, Never};
        // `--color` beats `[term] color`, and both beat `NO_COLOR`.
        assert!(colors(Some(Always), Some(Never), true, false));
        assert!(!colors(Some(Never), Some(Always), false, true));
        assert!(colors(None, Some(Always), true, false));
        assert!(!colors(None, Some(Never), false, true));
        // `auto`, given or configured, follows the terminal over `NO_COLOR`.
        assert!(colors(Some(Auto), None, true, true));
        assert!(!colors(Some(Auto), Some(Always), false, false));
        assert!(colors(None, Some(Auto), true, true));
        // With neither, `NO_COLOR` decides, then the terminal.
        assert!(!colors(None, None, true, true));
        assert!(colors(None, None, false, true));
        assert!(!colors(None, None, false, false));
    }

    /// `[term] color` ranks below `--color` and above `NO_COLOR`
    /// (`commands.md`, "Available for all commands").
    ///
    /// Mutation: let `[term] color` beat `--color`, let `NO_COLOR` beat
    /// `[term] color`, or ignore what `configure` is given; this test fails.
    #[test]
    fn the_configured_choice_ranks_between_the_flag_and_no_color() {
        let on = |given, configured, env: &[(&str, &str)], terminal| {
            let mut shell = shell(given, env, terminal).0;
            shell.configure(configured, false);
            shell.uses_color()
        };
        let no_color: &[(&str, &str)] = &[("NO_COLOR", "")];
        let (always, never) = (Some(ColorChoice::Always), Some(ColorChoice::Never));
        assert!(on(None, always, no_color, false));
        assert!(!on(None, never, &[], true));
        assert!(on(None, Some(ColorChoice::Auto), no_color, true));
        assert!(!on(never, always, &[], true));
        assert!(on(always, never, &[], false));
        assert!(!on(None, None, no_color, true));
        assert!(on(None, None, &[], true));
    }
}
