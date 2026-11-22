use super::*;
use crate::graph::StepKind;
use std::ffi::OsString;
use std::time::{Duration, SystemTime};

struct Case {
    _root: tempfile::TempDir,
    step: Step,
    input: PathBuf,
}

/// A compile of `a.cpp` into `a.obj` that has run and been recorded.
fn recorded() -> (Case, Records) {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("a.cpp");
    fs::write(&input, "int a;").unwrap();
    set_time(&input, 1_000);
    let output = root.path().join("a.obj");
    fs::write(&output, "obj").unwrap();
    let step = Step {
        kind: StepKind::Compile {
            source: input.clone(),
        },
        program: "cl.exe".into(),
        args: vec![OsString::from("/c")],
        output,
        after: Vec::new(),
        echo: None,
        headers: None,
    };
    let records = fresh();
    records.record(&step, std::slice::from_ref(&input)).unwrap();
    (
        Case {
            _root: root,
            step,
            input,
        },
        records,
    )
}

/// Another build's records, so nothing is known from this one.
fn fresh() -> Records {
    Records::new([OsStr::new("19.52"), OsStr::new("C:/inc")])
}

fn set_time(path: &Path, seconds: u64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

/// Nothing changed: the step need not run.
///
/// Mutation: always run; this test fails.
#[test]
fn an_unchanged_step_is_up_to_date() {
    let (case, _) = recorded();
    assert!(fresh().up_to_date(&case.step));
}

/// A new time with the same bytes is no change, and the record takes the
/// new time, so the bytes are not read again next time.
///
/// Mutation: judge by time alone, or leave the record as it was; this test
/// fails.
#[test]
fn a_new_time_with_the_same_bytes_is_no_change() {
    let (case, _) = recorded();
    set_time(&case.input, 2_000);
    assert!(fresh().up_to_date(&case.step));
    let record = read(&state_path(&case.step.output)).unwrap();
    assert_eq!(
        record.inputs[0].1.modified,
        Duration::from_secs(2_000).as_nanos()
    );
}

/// A file put back with an older time and other bytes of the same size,
/// as a restore from a backup does, is a change: the record is compared,
/// not the output's time.
///
/// Mutation: judge by "newer than the output", or skip the hash; this test
/// fails.
#[test]
fn an_older_file_with_other_bytes_is_a_change() {
    let (case, _) = recorded();
    fs::write(&case.input, "int b;").unwrap();
    set_time(&case.input, 500);
    assert!(!fresh().up_to_date(&case.step));
}

/// A different size is a change.
///
/// Mutation: compare only times; this test fails.
#[test]
fn a_different_size_is_a_change() {
    let (case, _) = recorded();
    fs::write(&case.input, "int a; int b;").unwrap();
    set_time(&case.input, 1_000);
    assert!(!fresh().up_to_date(&case.step));
}

/// Another argument, another compiler version or another environment, or
/// another cuno's record, means the step runs.
///
/// Mutation: ignore the arguments, the identity or the header line; this
/// test fails.
#[test]
fn another_command_or_cuno_is_a_change() {
    let (case, _) = recorded();
    let mut other = case.step.clone();
    other.args.push("/O2".into());
    assert!(!fresh().up_to_date(&other));
    let newer = Records::new([OsStr::new("19.53"), OsStr::new("C:/inc")]);
    assert!(!newer.up_to_date(&case.step));
    let sdk = Records::new([OsStr::new("19.52"), OsStr::new("C:/other")]);
    assert!(!sdk.up_to_date(&case.step));

    let path = state_path(&case.step.output);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replacen(HEADER, "cuno-state another", 1)).unwrap();
    assert!(!fresh().up_to_date(&case.step));
}

/// A missing output or input, a record forgotten before a run, or no
/// record at all, means the step runs.
///
/// Mutation: trust the record without the output, or keep a record across
/// a run that then fails; this test fails.
#[test]
fn what_is_missing_is_a_change() {
    let (case, _) = recorded();
    fs::remove_file(&case.step.output).unwrap();
    assert!(!fresh().up_to_date(&case.step));

    let (case, records) = recorded();
    records.forget(&case.step);
    assert!(!fresh().up_to_date(&case.step));

    let (case, _) = recorded();
    fs::remove_file(&case.input).unwrap();
    assert!(!fresh().up_to_date(&case.step));
}

/// A path that names inputs with a space, and one recorded twice, read
/// back as they were written.
///
/// Mutation: split the path at a space, or record a duplicate; this test
/// fails.
#[test]
fn paths_with_spaces_are_recorded_once() {
    let (case, records) = recorded();
    let spaced = case.input.with_file_name("inc dir.h");
    fs::write(&spaced, "#pragma once").unwrap();
    records
        .record(
            &case.step,
            &[case.input.clone(), spaced.clone(), spaced.clone()],
        )
        .unwrap();
    let record = read(&state_path(&case.step.output)).unwrap();
    let paths: Vec<&PathBuf> = record.inputs.iter().map(|(p, _)| p).collect();
    assert_eq!(paths, [&case.input, &spaced]);
    assert!(fresh().up_to_date(&case.step));
}
