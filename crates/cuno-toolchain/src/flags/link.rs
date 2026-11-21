//! The link step's arguments, row by row from `docs/en/specs/build.md`,
//! "Settings and flags". Linking goes through the compiler driver, so each
//! driver adds its own C runtime libraries.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{BuildSettings, OptLevel, ProfileSettings, Subsystem};
use crate::compiler::{CompilerKind, Family, Syntax};

/// What one link of the build is made from.
#[derive(Clone, Copy, Debug)]
pub struct LinkInput<'a> {
    /// The driver.
    pub kind: CompilerKind,
    /// The resolved build settings (`links`, `lib-dirs`, `link-flags`).
    pub build: &'a BuildSettings,
    /// The resolved profile (`debug-info`, `lto`, and `opt-level` for LTO).
    pub profile: &'a ProfileSettings,
    /// The executable's `[[bin]] subsystem`.
    pub subsystem: Subsystem,
}

/// The arguments for the driver to link `objects` into `exe`: the driver's
/// own switches, the objects and the output, then the linker's arguments
/// (after `/link` for MSVC and clang-cl), with `link-flags` last.
pub fn link_args(input: &LinkInput<'_>, objects: &[PathBuf], exe: &Path) -> Vec<OsString> {
    let build = input.build;
    let profile = input.profile;
    // lld optimises at level 2 when it runs LTO unless told the level.
    let lto_level = match profile.opt_level {
        OptLevel::O0 => "0",
        OptLevel::O1 => "1",
        OptLevel::O2 | OptLevel::Size => "2",
        OptLevel::O3 => "3",
    };
    let mut args: Vec<OsString> = Vec::new();
    let push = |args: &mut Vec<OsString>, flags: &[&str]| {
        args.extend(flags.iter().map(OsString::from));
    };

    match input.kind {
        CompilerKind::Msvc => push(&mut args, &["/nologo"]),
        CompilerKind::ClangCl => {
            push(&mut args, &["/nologo", "-fuse-ld=lld"]);
            if profile.lto {
                push(&mut args, &["-flto"]);
            }
        }
        CompilerKind::Clang => {
            // `-fms-runtime-lib` is left out: the objects already say which C
            // runtime they use, and the linker follows them.
            push(
                &mut args,
                &["--target=x86_64-pc-windows-msvc", "-fuse-ld=lld"],
            );
            if profile.debug_info {
                push(&mut args, &["-g"]);
            }
            if profile.lto {
                push(&mut args, &["-flto"]);
            }
        }
    }

    args.extend(objects.iter().map(OsString::from));

    if input.kind.syntax() == Syntax::Gnu {
        args.extend(["-o".into(), exe.into()]);
        for lib in &build.links {
            args.push(format!("-l{lib}").into());
        }
        for dir in &build.lib_dirs {
            let mut arg = OsString::from("-L");
            arg.push(dir);
            args.push(arg);
        }
        push(
            &mut args,
            match input.subsystem {
                Subsystem::Console => &["-Wl,/subsystem:console"][..],
                Subsystem::Windows => &["-Wl,/subsystem:windows", "-Wl,/entry:mainCRTStartup"],
            },
        );
        if profile.lto {
            args.push(format!("-Wl,/opt:lldlto={lto_level}").into());
        }
        // `-Xlinker`, not `-Wl,`, which would split a flag at its commas.
        for flag in &build.link_flags {
            args.extend(["-Xlinker".into(), flag.into()]);
        }
        return args;
    }

    let mut out = OsString::from("/Fe:");
    out.push(exe);
    args.push(out);
    // Everything after `/link` goes to the linker itself.
    args.push("/link".into());
    for lib in &build.links {
        args.push(format!("{lib}.lib").into());
    }
    for dir in &build.lib_dirs {
        let mut arg = OsString::from("/LIBPATH:");
        arg.push(dir);
        args.push(arg);
    }
    // Stated for console too: otherwise a program with `WinMain` and no
    // `main` is silently linked as a GUI program.
    push(
        &mut args,
        match input.subsystem {
            Subsystem::Console => &["/SUBSYSTEM:CONSOLE"][..],
            Subsystem::Windows => &["/SUBSYSTEM:WINDOWS", "/ENTRY:mainCRTStartup"],
        },
    );
    if profile.debug_info {
        push(&mut args, &["/DEBUG"]);
        if input.kind.family() == Family::Msvc {
            // link.exe links incrementally under /DEBUG; lld does not.
            push(&mut args, &["/INCREMENTAL:NO"]);
        }
    }
    if profile.lto {
        if input.kind.family() == Family::Msvc {
            push(&mut args, &["/LTCG"]);
        } else {
            args.push(format!("/opt:lldlto={lto_level}").into());
        }
    }
    args.extend(build.link_flags.iter().map(OsString::from));
    args
}

#[cfg(test)]
mod tests;
