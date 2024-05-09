//! Parsing Options.
//! `--detector-kind {kind}` or `-k`, currently support only deadlock

use clap::error::ErrorKind;

use clap::{Arg, Command};
use itertools::Itertools;
use rustc_session::EarlyErrorHandler;

#[derive(Debug)]
pub enum CrateNameList {
    White(Vec<String>),
    Black(Vec<String>),
}

impl Default for CrateNameList {
    fn default() -> Self {
        CrateNameList::White(Vec::new())
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DetectorKind {
    All,
    Deadlock,
    AtomicityViolation,
    SafeDrop,
    DataRace,
    // More to be supported.
}

fn make_options_parser() -> clap::Command {
    let parser = Command::new("PTA")
        .no_binary_name(true)
        .author("https://flml.tongji.edu.cn/")
        .version("v0.1.0")
        .arg(
            Arg::new("detector_kind")
                .short('k')
                .long("detector_kind")
                .help("The detector kind")
                .default_values(&["deadlock"])
                .value_parser(["deadlock", "race", "memory", "all"]),
            //.possible_values(),
        )
        .arg(
            Arg::new("output_dir")
                .short('o')
                .long("output_dir")
                .value_name("FILE")
                .help("Path to file where diagnostic information will be stored")
                .default_value("diagnostics.json"), // 默认的文件路径
        )
        .arg(
            Arg::new("main_crate").short('c').long("main_crate"), // 默认要建模的crate
        );
    parser
}

#[derive(Debug)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: Option<String>,
    pub crate_name: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Option::default(),
            crate_name: String::new(),
        }
    }
}

impl Options {
    pub fn parse_from_str(&mut self, s: &str, handler: &EarlyErrorHandler) -> Vec<String> {
        let args = shellwords::split(s).unwrap_or_else(|e| {
            handler.early_error(format!("Cannot parse argument string: {e:?}"))
        });
        self.parse_from_args(&args)
    }

    pub fn parse_from_args(&mut self, args: &[String]) -> Vec<String> {
        let mut pta_args_end = args.len();
        let mut rustc_args_start = 0;
        if let Some((p, _)) = args.iter().find_position(|s| s.as_str() == "--") {
            pta_args_end = p;
            rustc_args_start = p + 1;
        }
        let pta_args = &args[0..pta_args_end];
        let matches = if rustc_args_start == 0 {
            match make_options_parser().try_get_matches_from(pta_args.iter()) {
                Ok(matches) => {
                    rustc_args_start = args.len();
                    matches
                }
                Err(e) => match e.kind() {
                    ErrorKind::DisplayHelp => {
                        eprintln!("{e}");
                        return args.to_vec();
                    }
                    ErrorKind::UnknownArgument => {
                        return args.to_vec();
                    }
                    _ => {
                        eprintln!("{e}");
                        e.exit();
                    }
                },
            }
        } else {
            make_options_parser().get_matches_from(pta_args.iter())
        };
        // let app = make_options_parser();
        // let matches = app.try_get_matches_from(args.iter()).unwrap();
        //log::info!("matches: {:?}", matches);
        self.detector_kind = match matches.get_one::<String>("detector_kind").unwrap().as_str() {
            "deadlock" => DetectorKind::Deadlock,
            "atomicity_violation" => DetectorKind::AtomicityViolation,
            "safedrop" => DetectorKind::SafeDrop,
            "all" => DetectorKind::All,
            "datarace" => DetectorKind::DataRace,
            _ => DetectorKind::Deadlock,
        };
        if matches.contains_id("output_dir") {
            self.output = matches.get_one::<String>("output_dir").cloned();
        }

        if matches.contains_id("main_crate") {
            self.crate_name = matches.get_one::<String>("main_crate").unwrap().clone();
        }
        args[rustc_args_start..].to_vec()
    }
}
