//! The command-line surface must be byte-identical to the shipping build.
//!
//! `--help` and the error messages are the two things a differential harness
//! cannot check with a tolerance: they are exact strings a user reads and a
//! script may grep. They are also the easiest thing to drift while retyping a
//! 47-line help page into another language, so the Rust copy is compared
//! against the TypeScript source itself rather than against a snapshot of it.

/// The Rust binary's help page.
const HELP: &str = include_str!("../src/help.txt");
/// The shipping build's source, read at compile time.
const MAIN_TSX: &str = include_str!("../../../src/main.tsx");

/// Pull the `const HELP = ` template literal back out of the TypeScript.
fn typescript_help() -> &'static str {
    let start = MAIN_TSX
        .find("const HELP = `")
        .expect("the HELP literal should exist");
    let body = &MAIN_TSX[start + "const HELP = `".len()..];
    let end = body
        .find("`;")
        .expect("the HELP literal should be terminated");
    &body[..end]
}

#[test]
fn i24_the_help_page_matches_the_shipping_build_exactly() {
    let ts = typescript_help();
    assert_eq!(
        HELP, ts,
        "help text drifted from src/main.tsx — regenerate crates/sysmon/src/help.txt from it"
    );
    // A sanity floor, so an empty extraction cannot pass silently.
    assert!(
        HELP.lines().count() > 30,
        "only {} lines",
        HELP.lines().count()
    );
}

/// Every key the dashboard binds has to appear in `--help`, which is the same
/// obligation `test/keys-documented.test.ts` enforces on the TypeScript side.
/// An accelerator documented nowhere is an accelerator nobody has.
#[test]
fn every_bound_key_is_documented_in_the_help_page() {
    for key in [
        "left/right",
        "up/dn",
        "enter",
        "k",
        "/",
        "c m e",
        "r",
        "q",
        "+/-",
    ] {
        assert!(HELP.contains(key), "--help never mentions {key:?}");
    }
}

#[test]
fn the_help_page_documents_every_option() {
    for opt in [
        "--json",
        "--interval",
        "--energy=accurate",
        "--mock",
        "--help",
        "--version",
    ] {
        assert!(HELP.contains(opt), "--help never mentions {opt}");
    }
}

/// I-24: the exit statuses are part of the contract, and the help page says so.
#[test]
fn the_help_page_states_the_exit_statuses() {
    assert!(HELP.contains("Exit status"));
    for code in ["0  success", "1  could not run", "2  bad usage"] {
        assert!(HELP.contains(code), "--help never states {code:?}");
    }
}
