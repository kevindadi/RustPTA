#![feature(rustc_private)]
#![feature(box_patterns)]
#![warn(non_snake_case)]

pub mod callback;
pub mod concurrency;
// pub mod detect;
pub mod memory;
pub mod net;
pub mod options;
pub mod report;
pub mod translate;
pub mod util;

extern crate rustc_abi;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_symbol_mangling;

use log::debug;
use options::Options;
use rustc_session::{config::ErrorOutputType, EarlyDiagCtxt};

use crate::options::DetectorKind;

fn main() {
    let handler = EarlyDiagCtxt::new(ErrorOutputType::default());

    if std::env::var("RUSTC_LOG").is_ok() {
        rustc_driver::init_rustc_env_logger(&handler);
    }

    if std::env::var("PN_LOG").is_ok() {
        let e = env_logger::Env::new()
            .filter("PN_LOG")
            .write_style("PN_LOG_STYLE");
        env_logger::init_from_env(e);
    }

    let mut options = Options::default();

    let _ = options.parse_from_str(&std::env::var("PN_FLAGS").unwrap_or_default(), &handler);

    log::debug!("PN options from environment: {:?}", options);

    let mut args = std::env::args_os()
        .enumerate()
        .map(|(i, arg)| {
            arg.into_string().unwrap_or_else(|arg| {
                handler.early_fatal(format!("Argument {i} is not valid Unicode: {arg:?}"))
            })
        })
        .collect::<Vec<_>>();
    assert!(!args.is_empty());

    if args.len() > 1 && std::path::Path::new(&args[1]).file_stem() == Some("rustc".as_ref()) {
        args.remove(1);
    }

    let mut rustc_command_line_arguments: Vec<String> = args[1..].into();

    let result = rustc_driver::catch_fatal_errors(|| {
        rustc_command_line_arguments.insert(0, args[0].clone());

        let print: String = "--print=".into();
        if rustc_command_line_arguments
            .iter()
            .any(|arg| arg.starts_with(&print))
        {
        } else {
            let sysroot: String = "--sysroot".into();
            if !rustc_command_line_arguments
                .iter()
                .any(|arg| arg.starts_with(&sysroot))
            {
                rustc_command_line_arguments.push(sysroot);
                rustc_command_line_arguments.push(find_sysroot());
            }

            let always_encode_mir: String = "always-encode-mir".into();
            if !rustc_command_line_arguments
                .iter()
                .any(|arg| arg.ends_with(&always_encode_mir))
            {
                rustc_command_line_arguments.push("-Z".into());
                rustc_command_line_arguments.push(always_encode_mir);
            }
        }

        let mut callbacks = callback::PTACallbacks::new(options);
        debug!(
            "rustc_command_line_arguments {:?}",
            rustc_command_line_arguments
        );

        rustc_driver::run_compiler(&rustc_command_line_arguments, &mut callbacks);
    });

    let exit_code = match result {
        Ok(_) => rustc_driver::EXIT_SUCCESS,
        Err(_) => rustc_driver::EXIT_FAILURE,
    };
    std::process::exit(exit_code);
}

fn find_sysroot() -> String {
    let home = option_env!("RUSTUP_HOME");
    let toolchain = option_env!("RUSTUP_TOOLCHAIN");
    match (home, toolchain) {
        (Some(home), Some(toolchain)) => format!("{}/toolchains/{}", home, toolchain),
        _ => option_env!("RUST_SYSROOT")
            .expect(
                "Could not find sysroot. Specify the RUST_SYSROOT environment variable, \
                 or use rustup to set the compiler to use for LOCKBUD",
            )
            .to_owned(),
    }
}
