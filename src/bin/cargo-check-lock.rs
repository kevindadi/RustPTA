use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

const CARGO_PN_HELP: &str = r#"Petri Net-based Analysis Tool for Rust Programs

USAGE:
    cargo pn [OPTIONS] [-- <rustc-args>...]

OPTIONS:
    -h, --help                     Print help information
    -V, --version                  Print version information
    -m, --mode <TYPE>              Analysis mode:
                                   - deadlock: Deadlock detection
                                   - datarace: Data race detection
                                   - atomic: Atomicity Violation detection
                                   - pointsto: Standalone pointer analysis
                                   - [default: all]
    -p, --pn-crate <NAME>           Target crate for analysis(Only underlined links can be used)
    --pn-analysis-dir=<PATH>       Output path for analysis results [default: diagnostics.json]

VISUALIZATION OPTIONS:
        --viz-callgraph            Generate call graph visualization
        --viz-petrinet             Generate Petri net visualization
        --viz-stategraph           Generate state graph visualization
        --viz-unsafe               Generate unsafe operations report
        --viz-pointsto 

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

fn has_arg_flag(name: &str) -> bool {
    let mut args = std::env::args().take_while(|val| val != "--");
    args.any(|val| val == name)
}

fn find_pn_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let pn = dir.join("pn");
            if pn.exists() {
                return pn;
            }
            let pn = dir.join("pn.exe");
            if pn.exists() {
                return pn;
            }
        }
    }
    PathBuf::from("pn")
}

fn in_cargo_pta() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let split_pos = args.iter().position(|a| a == "--");
    let (flags, rest) = match split_pos {
        Some(pos) => (&args[..pos], &args[pos + 1..]),
        None => (args.as_slice(), &[][..]),
    };
    let flags_str = flags.join(" ");

    let file_arg = flags.iter().position(|a| a == "-f" || a == "--file");
    let single_file = file_arg.and_then(|i| flags.get(i + 1).cloned());

    if let Some(file) = single_file {
        let rustc = env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
        let pn_path = find_pn_binary();
        let mut cmd = Command::new(&pn_path);
        cmd.arg(&rustc);
        cmd.arg(&file);
        cmd.env("RUST_BACKTRACE", "full");
        cmd.env("PN_LOG", "info");
        cmd.env("PN_FLAGS", flags_str);
        let exit_status = cmd
            .spawn()
            .expect("could not run pn")
            .wait()
            .expect("failed to wait for pn?");
        if !exit_status.success() {
            std::process::exit(exit_status.code().unwrap_or(-1));
        }
        return;
    }

    let mut cmd = cargo();
    cmd.arg("build");
    cmd.env("RUSTC_WRAPPER", find_pn_binary());
    cmd.env("RUST_BACKTRACE", "full");
    cmd.env("PN_LOG", "info");
    cmd.env("PN_FLAGS", flags_str);
    cmd.args(rest);
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

    if let Some("pn") = std::env::args().nth(1).as_deref() {
        in_cargo_pta();
    }
}
