use std::env;
use std::ffi::OsString;
use std::process::Command;

const CARGO_PTA_HELP: &str = r#"Statically detect bugs on MIR"#;

fn show_help() {
    println!("{}", CARGO_PTA_HELP);
}

fn show_version() {
    println!("PTA 0.0.1");
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
    // Now we run `cargo build $FLAGS $ARGS`, giving the user the
    // change to add additional arguments. `FLAGS` is set to identify
    // this target. The user gets to control what gets actually passed to lockbud.
    let mut cmd = cargo();
    cmd.arg("build");
    cmd.env("RUSTC_WRAPPER", "pta");
    cmd.env("RUST_BACKTRACE", "full");
    cmd.env("PTA_LOG", "info");
    let args = std::env::args().skip(2);
    let mut flags = Vec::new();
    for arg in args {
        if arg == "--" {
            break;
        }
        flags.push(arg);
    }
    let flags = flags.join(" ");
    cmd.env("PTA_FLAGS", flags);
    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        std::process::exit(exit_status.code().unwrap_or(-1))
    };
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
    if let Some("pta") = std::env::args().nth(1).as_deref() {
        in_cargo_pta();
    }
}
