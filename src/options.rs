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

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetectorKind {
    All,
    Deadlock,
    AtomicityViolation,
    DataRace,
}

fn make_options_parser() -> clap::Command {
    let analysis_help = if cfg!(feature = "atomic-violation") {
        "Analysis mode: deadlock detection, data race detection, atomic violation detection (requires feature), or all."
    } else {
        "Analysis mode: deadlock detection, data race detection, or all."
    };

    let analysis_arg = {
        let arg = Arg::new("analysis_mode")
            .short('m')
            .long("mode")
            .help(analysis_help)
            .default_values(&["deadlock"])
            .hide_default_value(true);
        if cfg!(feature = "atomic-violation") {
            arg.value_parser(["deadlock", "datarace", "atomic", "all"])
        } else {
            arg.value_parser(["deadlock", "datarace", "all"])
        }
    };

    let parser = Command::new("PN")
        .no_binary_name(true)
        .version("v0.1.0")
        .arg(analysis_arg)
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
                    "dump_mir",
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
        )
        .arg(
            Arg::new("dump_mir")
                .long("viz-mir")
                .help("Generate MIR visualization (dot format)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("stop_after")
                .long("stop-after")
                .value_name("STAGE")
                .help("Stop analysis after specified stage: mir, callgraph, petrinet, stategraph")
                .value_parser(["mir", "callgraph", "petrinet", "stategraph"]),
        );
    parser
}
#[derive(Debug, Clone)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: Option<PathBuf>,
    pub crate_name: String,
    pub dump_options: DumpOptions,
    pub stop_after: StopAfter,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Option::default(),
            crate_name: String::new(),
            dump_options: DumpOptions::default(),
            stop_after: StopAfter::None,
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
    pub dump_mir: bool,
}

/// 流水线停止点，用于调试
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopAfter {
    None,
    AfterMir,        // 在 MIR 输出后停止
    AfterCallGraph,  // 在调用图构建后停止
    AfterPetriNet,   // 在 Petri 网构建后停止
    AfterStateGraph, // 在状态图构建后停止
}

impl Default for DumpOptions {
    fn default() -> Self {
        DumpOptions {
            dump_call_graph: false,
            dump_petri_net: false,
            dump_state_graph: false,
            dump_unsafe_info: false,
            dump_points_to: false,
            dump_mir: false,
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

        if matches!(self.detector_kind, DetectorKind::AtomicityViolation)
            && !cfg!(feature = "atomic-violation")
        {
            log::warn!("未启用 `atomic-violation` feature, 自动回退至死锁检测。");
            self.detector_kind = DetectorKind::Deadlock;
        }

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
            dump_mir: matches.get_flag("dump_mir"),
        };

        self.stop_after = match matches.get_one::<String>("stop_after") {
            Some(stage) => match stage.as_str() {
                "mir" => StopAfter::AfterMir,
                "callgraph" => StopAfter::AfterCallGraph,
                "petrinet" => StopAfter::AfterPetriNet,
                "stategraph" => StopAfter::AfterStateGraph,
                _ => StopAfter::None,
            },
            None => StopAfter::None,
        };

        rustc_args.to_vec()
    }
}
