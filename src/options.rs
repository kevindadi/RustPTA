use std::path::PathBuf;

use clap::error::ErrorKind;

use crate::config::PnConfig;
use clap::{Arg, ArgGroup, Command};
use rustc_session::EarlyDiagCtxt;
#[derive(Debug, Clone)]
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
    PointsTo,
}

fn make_options_parser() -> clap::Command {
    let analysis_help = if cfg!(feature = "atomic-violation") {
        "Analysis mode: deadlock, datarace, atomic (requires feature), all, or pointsto (standalone pointer analysis)."
    } else {
        "Analysis mode: deadlock, datarace, all, or pointsto (standalone pointer analysis)."
    };

    let analysis_arg = {
        let arg = Arg::new("analysis_mode")
            .short('m')
            .long("mode")
            .help(analysis_help)
            .default_values(&["deadlock"])
            .hide_default_value(true);
        if cfg!(feature = "atomic-violation") {
            arg.value_parser(["deadlock", "datarace", "atomic", "all", "pointsto"])
        } else {
            arg.value_parser(["deadlock", "datarace", "all", "pointsto"])
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
            Arg::new("config_file")
                .long("config")
                .value_name("FILE")
                .help("Path to configuration file (default: pn.toml)"),
        )
        .arg(
            Arg::new("target_crate")
                .short('p')
                .long("pn-crate")
                .help("Target crate for analysis (required for cargo; optional for single file)"),
        )
        .arg(
            Arg::new("input_file")
                .short('f')
                .long("file")
                .value_name("FILE")
                .help("Single .rs file to analyze (use with rustc invocation)"),
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
                .help("Stop analysis after specified stage: mir, callgraph, pointsto, petrinet, stategraph")
                .value_parser(["mir", "callgraph", "pointsto", "petrinet", "stategraph"]),
        )
        .arg(
            Arg::new("state_limit")
                .long("state-limit")
                .value_name("N")
                .help("Max states to explore in reachability (default: 50000, 0 = unlimited)")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("full")
                .long("full")
                .help("Translate all functions (disables entry-reachable and concurrent-roots filtering)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("crate_whitelist")
                .long("crate-whitelist")
                .value_name("CRATES")
                .help("Comma-separated crate names to analyze (only these crates)")
                .value_delimiter(','),
        )
        .arg(
            Arg::new("crate_blacklist")
                .long("crate-blacklist")
                .value_name("CRATES")
                .help("Comma-separated crate names to exclude from analysis")
                .value_delimiter(','),
        )
        .arg(
            Arg::new("no_reduce")
                .long("no-reduce")
                .help("Disable Petri net reduction before reachability")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("por")
                .long("por")
                .help("Enable partial order reduction (POR) to reduce equivalent interleavings")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no_concurrent_roots")
                .long("no-concurrent-roots")
                .help("Disable translating functions that use locks/atomics/condvars/channels (and their callees)")
                .action(clap::ArgAction::SetTrue),
        );
    parser
}
#[derive(Debug, Clone)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: Option<PathBuf>,
    pub crate_name: String,
    pub crate_filter: CrateNameList,
    pub dump_options: DumpOptions,
    pub stop_after: StopAfter,
    pub config: PnConfig,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Option::default(),
            crate_name: String::new(),
            crate_filter: CrateNameList::default(),
            dump_options: DumpOptions::default(),
            stop_after: StopAfter::None,
            config: PnConfig::default(),
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

/// 流水线停止点,用于调试
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopAfter {
    None,
    AfterMir,        // 在 MIR 输出后停止
    AfterCallGraph,  // 在调用图构建后停止
    AfterPointsTo,   // 在指针分析后停止
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
            "pointsto" => DetectorKind::PointsTo,
            _ => DetectorKind::Deadlock,
        };

        if matches!(self.detector_kind, DetectorKind::AtomicityViolation)
            && !cfg!(feature = "atomic-violation")
        {
            log::warn!("未启用 atomic-violation feature, 自动回退至死锁检测.");
            self.detector_kind = DetectorKind::Deadlock;
        }

        self.crate_name = matches
            .get_one::<String>("target_crate")
            .cloned()
            .or_else(|| {
                matches.get_one::<String>("input_file").map(|f| {
                    std::path::Path::new(f)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("main")
                        .to_string()
                })
            })
            .unwrap_or_else(|| "main".to_string());
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
                "pointsto" => StopAfter::AfterPointsTo,
                "petrinet" => StopAfter::AfterPetriNet,
                "stategraph" => StopAfter::AfterStateGraph,
                _ => StopAfter::None,
            },
            None => StopAfter::None,
        };

        let config_path = matches
            .get_one::<String>("config_file")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pn.toml"));

        match PnConfig::load_from_file(&config_path) {
            Ok(cfg) => self.config = cfg,
            Err(e) => {
                log::warn!("Failed to load config from {:?}: {}", config_path, e);
            }
        }

        if let Some(&n) = matches.get_one::<usize>("state_limit") {
            self.config.state_limit = if n == 0 { None } else { Some(n) };
        }
        if matches.get_flag("full") {
            self.config.entry_reachable = false;
        }
        if let Some(crates) = matches.get_many::<String>("crate_whitelist") {
            self.crate_filter = CrateNameList::White(crates.cloned().collect());
        } else if let Some(crates) = matches.get_many::<String>("crate_blacklist") {
            self.crate_filter = CrateNameList::Black(crates.cloned().collect());
        }
        if matches.get_flag("no_reduce") {
            self.config.reduce_net = false;
        }
        if matches.get_flag("por") {
            self.config.por_enabled = true;
        }
        if matches.get_flag("no_concurrent_roots") {
            self.config.translate_concurrent_roots = false;
        }

        rustc_args.to_vec()
    }

    /// 从 rustc 命令行参数中推断 crate 名（当 -p 和 -f 均未指定时）
    pub fn infer_crate_name_from_rustc_args(&mut self, rustc_args: &[String]) {
        if !self.crate_name.is_empty() && self.crate_name != "main" {
            return;
        }
        // 查找 --crate-name 参数
        if let Some(pos) = rustc_args.iter().position(|a| a == "--crate-name") {
            if let Some(name) = rustc_args.get(pos + 1) {
                self.crate_name = name.clone();
                return;
            }
        }
        // 查找 .rs 输入文件
        for arg in rustc_args {
            if arg.ends_with(".rs") {
                self.crate_name = std::path::Path::new(arg)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("main")
                    .to_string();
                return;
            }
        }
    }
}
