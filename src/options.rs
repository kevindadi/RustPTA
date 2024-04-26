//! Parsing Options.
//! `--detector-kind {kind}` or `-k`, currently support only deadlock

use clap::{Arg, Command};
use std::error::Error;

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
    Memory,
    Panic,
    // More to be supported.
}

fn make_options_parser() -> clap::Command {
    let parser = Command::new("PTA")
        .no_binary_name(true)
        .author("https://flml.tongji.edu.cn/")
        .version("v0.1.0")
        .arg(
            Arg::new("kind")
                .short('k')
                .long("detector-kind")
                .help("The detector kind")
                .default_values(&["deadlock"])
                .value_parser(["deadlock", "race", "memory", "all"]),
            //.possible_values(),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Path to file where diagnostic information will be stored")
                .default_value("diagnostics.json"), // 默认的文件路径
        )
        .arg(
            Arg::new("crate")
                .short('c')
                .long("crate-name")
                .default_value("test"), // 默认的文件路径
        );
    parser
}

#[derive(Debug)]
pub struct Options {
    pub detector_kind: DetectorKind,
    pub output: String,
    pub crate_name: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detector_kind: DetectorKind::Deadlock,
            output: Default::default(),
            crate_name: String::new(),
        }
    }
}

impl Options {
    pub fn parse_from_str(s: &str) -> Result<Self, Box<dyn Error>> {
        let flags = shellwords::split(s)?;
        Self::parse_from_args(&flags)
    }

    pub fn parse_from_args(flags: &[String]) -> Result<Self, Box<dyn Error>> {
        let app = make_options_parser();
        let matches = app.try_get_matches_from(flags.iter())?;
        let detector_kind = match matches.get_one::<&str>("kind") {
            Some(&"deadlock") => DetectorKind::Deadlock,
            Some(&"atomicity_violation") => DetectorKind::AtomicityViolation,
            Some(&"memory") => DetectorKind::Memory,
            Some(&"all") => DetectorKind::All,
            Some(&"panic") => DetectorKind::Panic,
            _ => return Err("UnsupportedDetectorKind")?,
        };

        let output = matches.get_one::<String>("output").unwrap().to_string();

        let crate_name = matches.get_one::<String>("cn").unwrap().to_string();
        Ok(Options {
            detector_kind,
            output,
            crate_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_from_str_err() {
        let options = Options::parse_from_str("-k unknown -b -l cc,tokio_util,indicatif");
        assert!(options.is_err());
    }

    #[test]
    fn test_parse_from_args_err() {
        let options = Options::parse_from_args(&[
            "-k".to_owned(),
            "unknown".to_owned(),
            "-b".to_owned(),
            "-l".to_owned(),
            "cc,tokio_util,indicatif".to_owned(),
        ]);
        assert!(options.is_err());
    }
}
