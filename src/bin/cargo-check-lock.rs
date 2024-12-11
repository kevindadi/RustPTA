use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use RustPTA::parse_thread_sanitizer_report;

const CARGO_PTA_HELP: &str = r#"PetriNet for checking deadlock and data race"#;

fn show_help() {
    println!("{}", CARGO_PTA_HELP);
}

fn show_version() {
    println!("PetriNet for checking deadlock and data race 0.0.1");
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
