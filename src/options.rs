use std::path::PathBuf;

use clap::error::ErrorKind;

use clap::{Arg, ArgGroup, Command};
use rustc_session::EarlyDiagCtxt;

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

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DetectorKind {
    All,
    Deadlock,
    AtomicityViolation,
    DataRace,
}

fn make_options_parser() -> clap::Command {
    let parser = Command::new("PN")
        .no_binary_name(true)
        .version("v0.1.0")
        .arg(
            Arg::new("analysis_mode")
                .short('m')
                .long("mode")
                .help("Analysis mode: deadlock detection, data race detection, etc.")
                .default_values(&["deadlock"])
                .value_parser(["deadlock", "datarace", "atomic", "all"])
                .hide_default_value(true),
        )
        .arg(
            Arg::new("diagnostics_output")
                .long("pn-analysis-dir")
                .value_name("PATH")
                .help("Directory for Petri net analysis outputs (default: ./tmp/<crate_name>)")
                .default_value("./tmp"),
        )
        .arg(
            Arg::new("target_crate")
                .short('p')
                .long("pn-crate")
                .help("Target crate for analysis"),
        )
        .group(
            ArgGroup::new("visualization")
                .args([
                    "dump_callgraph",
                    "dump_petrinet",
                    "dump_stategraph",
                    "dump_unsafe",
                    "dump_points_to",
                ])
                .multiple(true),
        )
        .arg(
            Arg::new("dump_callgraph")
                .long("viz-callgraph")
                .help("Generate call graph visualization")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dump_petrinet")
                .long("viz-petrinet")
                .help("Generate Petri net visualization")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dump_stategraph")
                .long("viz-stategraph")
                .help("Generate state graph visualization")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dump_unsafe")
                .long("viz-unsafe")
                .help("Generate unsafe operations report")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dump_points_to")
                .long("viz-pointsto")
                .help("Generate points-to relations report")
                .action(clap::ArgAction::SetTrue),
        );
    parser
}
#[derive(Debug, Clone)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: Option<PathBuf>,
    pub crate_name: String,
    pub dump_options: DumpOptions,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Option::default(),
            crate_name: String::new(),
            dump_options: DumpOptions::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DumpOptions {
    pub dump_call_graph: bool,
    pub dump_petri_net: bool,
    pub dump_state_graph: bool,
    pub dump_unsafe_info: bool,
    pub dump_points_to: bool,
}

impl Default for DumpOptions {
    fn default() -> Self {
        DumpOptions {
            dump_call_graph: false,
            dump_petri_net: false,
            dump_state_graph: false,
            dump_unsafe_info: false,
            dump_points_to: false,
        }
    }
}

impl Options {
    pub fn parse_from_str(&mut self, s: &str, handler: &EarlyDiagCtxt) -> Vec<String> {
        let args = shellwords::split(s).unwrap_or_else(|e| {
            handler.early_fatal(format!("Cannot parse argument string: {e:?}"))
        });

        self.parse_from_args(&args)
    }

    pub fn parse_from_args(&mut self, args: &[String]) -> Vec<String> {
        let (pn_args, rustc_args) = match args.iter().position(|s| s == "--") {
            Some(pos) => (&args[..pos], &args[pos + 1..]),
            None => (args, &[][..]),
        };

        let matches = make_options_parser()
            .try_get_matches_from(pn_args.iter())
            .unwrap_or_else(|e| match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::UnknownArgument => {
                    eprintln!("{e}");
                    e.exit();
                }
                _ => {
                    eprintln!("{e}");
                    e.exit();
                }
            });

        self.detector_kind = match matches.get_one::<String>("analysis_mode").unwrap().as_str() {
            "deadlock" => DetectorKind::Deadlock,
            "atomic" => DetectorKind::AtomicityViolation,
            "all" => DetectorKind::All,
            "datarace" => DetectorKind::DataRace,
            _ => DetectorKind::Deadlock,
        };

        self.crate_name = matches
            .get_one::<String>("target_crate")
            .expect("The target crate must be declared and linked with an underscore.")
            .clone();
        self.output = matches
            .get_one::<String>("diagnostics_output")
            .cloned()
            .map(PathBuf::from);

        self.dump_options = DumpOptions {
            dump_call_graph: matches.get_flag("dump_callgraph"),
            dump_petri_net: matches.get_flag("dump_petrinet"),
            dump_state_graph: matches.get_flag("dump_stategraph"),
            dump_unsafe_info: matches.get_flag("dump_unsafe"),
            dump_points_to: matches.get_flag("dump_points_to"),
        };

        rustc_args.to_vec()
    }
}
