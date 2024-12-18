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
    // More to be supported.
}

fn make_options_parser() -> clap::Command {
    let parser = Command::new("PN")
        .no_binary_name(true)
        .author("https://flml.tongji.edu.cn/")
        .version("v0.1.0")
        .arg(
            Arg::new("analysis_mode")
                .short('m')
                .long("mode")
                .help("Analysis mode: deadlock detection, data race detection, etc.")
                .default_values(&["deadlock"])
                .value_parser(["deadlock", "datarace", "memory", "all"])
                .hide_default_value(true),
        )
        .arg(
            Arg::new("diagnostics_output")
                .short('o')
                .long("output")
                .value_name("PATH")
                .help("Output path for analysis diagnostics and reports")
                .default_value("diagnostics.json"),
        )
        .arg(
            Arg::new("target_crate")
                .short('t')
                .long("target")
                .help("Target crate for analysis")
                .required(true),
        )
        .arg(
            Arg::new("crate_type")
                .long("type")
                .help("Target crate type")
                .value_parser(["binary", "library"])
                .default_value("binary"),
        )
        .arg(
            Arg::new("api_spec")
                .long("api-spec")
                .value_name("PATH")
                .help("Path to library API specification file"),
        )
        // Visualization options group
        .group(
            ArgGroup::new("visualization")
                .args([
                    "dump_callgraph",
                    "dump_petrinet",
                    "dump_stategraph",
                    "dump_unsafe",
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
        );
    parser
}
#[derive(Debug, Clone)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: Option<String>,
    pub crate_name: String,
    pub crate_type: OwnCrateType,      // 区分 bin/lib lib 绑定libapis
    pub lib_apis_path: Option<String>, // lib APIs 文件路径
    pub dump_options: DumpOptions,     // dump 相关选项
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Option::default(),
            crate_name: String::new(),
            crate_type: OwnCrateType::Bin,
            lib_apis_path: None,
            dump_options: DumpOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnCrateType {
    Bin,
    Lib,
}

#[derive(Debug, Clone)]
pub struct DumpOptions {
    pub dump_call_graph: bool,
    pub dump_petri_net: bool,
    pub dump_state_graph: bool,
    pub dump_unsafe_info: bool,
}

impl Default for DumpOptions {
    fn default() -> Self {
        DumpOptions {
            dump_call_graph: false,
            dump_petri_net: false,
            dump_state_graph: false,
            dump_unsafe_info: false,
        }
    }
}

impl Options {
    pub fn parse_from_str(&mut self, s: &str, handler: &EarlyDiagCtxt) -> Vec<String> {
        // 使用 shellwords 解析字符串为参数列表
        let args = shellwords::split(s).unwrap_or_else(|e| {
            handler.early_fatal(format!("Cannot parse argument string: {e:?}"))
        });
        // 调用 parse_from_args 进行进一步解析
        self.parse_from_args(&args)
    }

    pub fn parse_from_args(&mut self, args: &[String]) -> Vec<String> {
        // 分割 PN 和 rustc 参数
        let (pn_args, rustc_args) = match args.iter().position(|s| s == "--") {
            Some(pos) => (&args[..pos], &args[pos + 1..]),
            None => (args, &[][..]),
        };

        // 解析 PN 参数
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

        // 更新 Options 结构体
        self.detector_kind = match matches.get_one::<String>("analysis_mode").unwrap().as_str() {
            "deadlock" => DetectorKind::Deadlock,
            "atomicity_violation" => DetectorKind::AtomicityViolation,
            "all" => DetectorKind::All,
            "datarace" => DetectorKind::DataRace,
            _ => DetectorKind::Deadlock,
        };

        self.output = matches.get_one::<String>("diagnostics_output").cloned();
        self.crate_name = matches.get_one::<String>("target_crate").unwrap().clone();

        // 解析crate类型
        self.crate_type = match matches.get_one::<String>("crate_type").unwrap().as_str() {
            "library" => OwnCrateType::Lib,
            _ => OwnCrateType::Bin,
        };

        // 验证库API的正确性
        match self.crate_type {
            OwnCrateType::Lib => {
                self.lib_apis_path = Some(matches.get_one::<String>("api_spec").cloned().ok_or_else(|| {
                    eprintln!("Error: Library crate requires API specification file path (--api-spec)");
                    eprintln!("Usage: --api-spec <PATH> specifies the path to library API configuration");
                    std::process::exit(1);
                }).unwrap());
            }
            OwnCrateType::Bin => {
                self.lib_apis_path = matches.get_one::<String>("api_spec").cloned();
            }
        }

        // 更新可视化选项
        self.dump_options = DumpOptions {
            dump_call_graph: matches.get_flag("dump_callgraph"),
            dump_petri_net: matches.get_flag("dump_petrinet"),
            dump_state_graph: matches.get_flag("dump_stategraph"),
            dump_unsafe_info: matches.get_flag("dump_unsafe"),
        };

        // 返回 rustc 参数
        rustc_args.to_vec()
    }
}
