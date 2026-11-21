//! The link step's arguments, row by row from `docs/en/specs/build.md`,
//! "Settings and flags". Linking goes through the compiler driver, so each
//! driver adds its own C runtime libraries.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{BuildSettings, ProfileSettings, Runtime, Subsystem};
use crate::select::CompilerKind;

/// What one link of the build is made from.
#[derive(Clone, Copy, Debug)]
pub struct LinkInput<'a> {
    /// The driver.
    pub kind: CompilerKind,
    /// The resolved build settings (`links`, `lib-dirs`, `link-flags`,
    /// `runtime`).
    pub build: &'a BuildSettings,
    /// The resolved profile (`debug-info`, `lto`).
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
            push(
                &mut args,
                &[
                    "--target=x86_64-pc-windows-msvc",
                    "-fuse-ld=lld",
                    match build.runtime {
                        Runtime::Dynamic => "-fms-runtime-lib=dll",
                        Runtime::Static => "-fms-runtime-lib=static",
                    },
                ],
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

    if input.kind == CompilerKind::Clang {
        args.extend(["-o".into(), exe.into()]);
        for lib in &build.links {
            args.push(format!("-l{lib}").into());
        }
        for dir in &build.lib_dirs {
            let mut arg = OsString::from("-L");
            arg.push(dir);
            args.push(arg);
        }
        if input.subsystem == Subsystem::Windows {
            push(
                &mut args,
                &["-Wl,/subsystem:windows", "-Wl,/entry:mainCRTStartup"],
            );
        }
        for flag in &build.link_flags {
            args.push(format!("-Wl,{flag}").into());
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
    if input.subsystem == Subsystem::Windows {
        push(&mut args, &["/SUBSYSTEM:WINDOWS", "/ENTRY:mainCRTStartup"]);
    }
    if profile.debug_info {
        push(&mut args, &["/DEBUG"]);
    }
    if profile.lto && input.kind == CompilerKind::Msvc {
        push(&mut args, &["/LTCG"]);
    }
    args.extend(build.link_flags.iter().map(OsString::from));
    args
}

#[cfg(test)]
mod tests;
