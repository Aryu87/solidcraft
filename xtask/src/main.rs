//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`) are invoked through
//! `std::process::Command`.

mod assets;
mod book;
mod features;
mod layers;
mod licences;
mod test;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  assets          check that every non-code asset file is attributed in ATTRIBUTION.md
  features        list SolidCraft's commands by area in docs/features.md (from the registry)
  oracle          writes the docs/oracle.md placeholder (the oracle is being rebuilt, #292)
  step-corpus [--iges]
                  writes the docs/step-import.md (--iges: docs/iges-import.md) placeholder
                  (the import corpus is being rebuilt, #292)
  book [--no-build]
                  generate docs/book's command reference and screenshots, then mdbook build
                  into target/book
  licences [--check]
                  write THIRD-PARTY-LICENSES.txt (the licences of every shipped crate, embedded
                  in the programs); --check fails when it is out of date
  test [CARGO ARGS] [-- TEST ARGS]
                  build the tests with the ci profile, then run every test binary at once
                  (and the doc tests alongside)
  wasm            cargo check every library crate and the web app for wasm32-unknown-unknown
  ci              fmt --check, assets, licences --check, layers, clippy -D warnings, then the tests
                  with the wasm check alongside (stops at the first failing step)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("layers") => cmd_layers(),
        Some("assets") => assets::run(&root()),
        Some("features") => features::run(&root()),
        Some("oracle") => placeholder("docs/oracle.md", "Modelling oracle"),
        Some("step-corpus") if rest.contains(&"--iges") => placeholder("docs/iges-import.md", "IGES import corpus"),
        Some("step-corpus") => placeholder("docs/step-import.md", "STEP import corpus"),
        Some("book") => book::run(&root(), !rest.contains(&"--no-build")),
        Some("licences") => licences::run(&root(), rest.contains(&"--check")),
        Some("test") => {
            let (cargo_args, test_args) = rest.iter().position(|a| *a == "--").map_or((&rest[..], &[][..]), |i| (&rest[..i], &rest[i + 1..]));
            test::run(cargo_args, test_args, vec![test::doc_tests(cargo_args, test_args)])
        }
        Some("ci") => cmd_ci(),
        Some("wasm") => cmd_wasm(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// The reference-results pages are being rebuilt on analytic and FreeCAD reference values
/// (#292); until then `oracle` and `step-corpus` write a placeholder. A page with a
/// `## Reference corpus` section (step-import.md) keeps everything above it.
fn placeholder(path: &str, title: &str) -> Result<(), String> {
    const SECTION: &str = "## Reference corpus";
    const NOTE: &str = "Being rebuilt on analytic and FreeCAD reference values; see \
                        [#292](https://github.com/bherbruck/solidcraft/issues/292).\n";
    let file = root().join(path);
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let md = match old.find(SECTION) {
        Some(i) => format!("{}{SECTION}\n\n{NOTE}", &old[..i]),
        None => format!("# {title}\n\n{NOTE}"),
    };
    std::fs::write(&file, md).map_err(|e| format!("{path}: {e}"))?;
    println!("{title}: being rebuilt (#292) → {path}");
    Ok(())
}

/// Never break wasm: every layered crate (everything below the apps) and the web app must
/// build for the browser.
fn cmd_wasm() -> Result<(), String> {
    let (c, n) = wasm_check()?;
    run(c, &format!("cargo check --target wasm32-unknown-unknown ({n} crates)"))?;
    println!("wasm: {n} crates build for wasm32-unknown-unknown");
    Ok(())
}

/// The wasm check and how many crates it covers. It uses the ci profile so the host build
/// scripts and proc macros built for clippy and the tests are reused.
fn wasm_check() -> Result<(Command, usize), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    let mut set: Vec<String> =
        crates.iter().filter(|c| matches!(layers::classify(&c.name), Some(layers::Class::Layer(_)))).map(|c| c.name.clone()).collect();
    set.push("solvecraft-web".into());
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--profile", "ci"]);
    for p in &set {
        c.args(["-p", p]);
    }
    Ok((c, set.len()))
}

/// The quick gates first, so they fail fast; the wasm check (and doc tests) run while the test
/// binaries do, which leaves cargo's build lock free.
fn cmd_ci() -> Result<(), String> {
    let mut c = cargo();
    c.args(["fmt", "--all", "--", "--check"]);
    run(c, "cargo fmt --check")?;
    assets::run(&root())?;
    licences::run(&root(), true)?;
    cmd_layers()?;
    let mut c = cargo();
    c.args(["clippy", "--workspace", "--all-targets", "--profile", "ci", "--", "-D", "warnings"]);
    run(c, "cargo clippy -D warnings")?;
    let (wasm, n) = wasm_check()?;
    test::run(&[], &[], vec![(format!("wasm32-unknown-unknown check ({n} crates)"), wasm), test::doc_tests(&[], &[])])?;
    eprintln!("ci: all gates passed");
    Ok(())
}
