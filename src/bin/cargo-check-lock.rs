use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use RustPTA::parse_thread_sanitizer_report;

const CARGO_PN_HELP: &str = r#"Petri Net-based Analysis Tool for Rust Programs

USAGE:
    cargo pn [OPTIONS] [-- <rustc-args>...]

OPTIONS:
    -h, --help                      Print help information
    -V, --version                   Print version information
    -m, --mode <TYPE>              Analysis mode:
                                   - deadlock: Deadlock detection
                                   - datarace: Data race detection
                                   - memory: Memory safety analysis
                                   - all: Run all analyses [default: deadlock]
    -t, --target <NAME>            Target crate for analysis
    -o, --output <PATH>            Output path for analysis results [default: diagnostics.json]
        --type <TYPE>              Target crate type (binary/library) [default: binary]
        --api-spec <PATH>          Path to library API specification file

VISUALIZATION OPTIONS:
        --viz-callgraph            Generate call graph visualization
        --viz-petrinet            Generate Petri net visualization
        --viz-stategraph          Generate state graph visualization
        --viz-unsafe              Generate unsafe operations report

EXAMPLES:
    cargo pn -m datarace -t my_crate
    cargo pn -m all -o results.json --viz-petrinet
    cargo pn -t my_lib --type library --api-spec apis.json
"#;

fn show_help() {
    println!("{}", CARGO_PN_HELP);
}

fn show_version() {
    println!("PetriNet for detecting concurrency bugs 0.0.1");
}

fn cargo() -> Command {
    Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")))
}

// Determines whether a `--flag` is present.
fn has_arg_flag(name: &str) -> bool {
    let mut args = std::env::args().take_while(|val| val != "--");
    args.any(|val| val == name)
}

fn in_cargo_pta() {
    let mut cmd = cargo();
    cmd.arg("build");
    cmd.env("RUSTC_WRAPPER", "pn");
    cmd.env("RUST_BACKTRACE", "full");
    cmd.env("PN_LOG", "info");
    let args = std::env::args().skip(2);
    let mut flags = Vec::new();
    for arg in args {
        if arg == "--" {
            break;
        }
        flags.push(arg);
    }
    let flags = flags.join(" ");
    cmd.env("PN_FLAGS", flags);
    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        std::process::exit(exit_status.code().unwrap_or(-1))
    };
}

#[allow(dead_code)]
fn cargo_sanitizer() {
    let mut cmd = cargo();
    cmd.arg("run");
    cmd.env("RUSTFLAGS", "-Zsanitizer=thread");
    let args = std::env::args().skip(2);
    let mut flags = Vec::new();
    for arg in args {
        if arg == "--" {
            break;
        }
        flags.push(arg);
    }

    let output = cmd
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);

    let reports = parse_thread_sanitizer_report(&stderr);
    let mut file = std::fs::File::create("data_race_report.txt").unwrap();

    for report in reports {
        writeln!(file, "{}", report).unwrap();
    }
}

fn main() {
    if has_arg_flag("--help") || has_arg_flag("-h") {
        show_help();
        return;
    }

    if has_arg_flag("--version") || has_arg_flag("-V") {
        show_version();
        return;
    }

    // let args: Vec<String> = std::env::args().collect();
    // for arg in &args {
    //     if arg == "datarace" {
    //         cargo_sanitizer();
    //         return;
    //     }
    // }

    if let Some("pn") = std::env::args().nth(1).as_deref() {
        in_cargo_pta();
    }
}
