use petgraph::{visit::EdgeRef, Direction};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::{
    fs::File,
    io::{self, Write},
};

use crate::extern_tools::normalize_name;
use crate::graph::net_structure::PetriNetNode;
use crate::graph::pn::PetriNet;
use crate::report::{DeadlockReport, DeadlockTrace};

#[derive(Debug, Serialize, Deserialize)]
pub struct LolaResult {
    pub has_deadlock: bool,
    pub execution_time: f64,
    pub error: Option<String>,
    pub deadlock_trace: Option<Vec<String>>,
    pub formula_result: Option<String>,
}

#[derive(Debug)]
pub struct LolaAnalyzer {
    lola_path: String,
    net_file: String,
    pub output_directory: PathBuf,
}

impl LolaAnalyzer {
    pub fn new(lola_path: String, net_file: String, output_directory: PathBuf) -> Self {
        Self {
            lola_path,
            net_file,
            output_directory,
        }
    }

    pub fn convert_to_lola(&self, petri_net: &PetriNet) -> String {
        let mut places = Vec::<String>::new();
        let mut transitions = Vec::<String>::new();
        let mut markings = Vec::<String>::new();

        for index in petri_net.net.node_indices() {
            if let PetriNetNode::P(place) = &petri_net.net[index] {
                let normalized_name = normalize_name(&place.name);
                places.push(normalized_name.clone());
                let tokens = place.tokens.borrow();
                if *tokens > 0 {
                    markings.push(format!("{}: {}", normalized_name, *tokens));
                }
            }
        }

        for index in petri_net.net.node_indices() {
            if let PetriNetNode::T(transition) = &petri_net.net[index] {
                let mut consume = Vec::new();
                let mut produce = Vec::new();

                for edge in petri_net.net.edges_directed(index, Direction::Incoming) {
                    if let PetriNetNode::P(place) = &petri_net.net[edge.source()] {
                        consume.push(format!(
                            "{}: {}",
                            normalize_name(&place.name),
                            edge.weight().label
                        ));
                    }
                }

                for edge in petri_net.net.edges_directed(index, Direction::Outgoing) {
                    if let PetriNetNode::P(place) = &petri_net.net[edge.target()] {
                        produce.push(format!(
                            "{}: {}",
                            normalize_name(&place.name),
                            edge.weight().label
                        ));
                    }
                }

                let transition_str = format!(
                    "TRANSITION {}\n    CONSUME {};\n    PRODUCE {};",
                    normalize_name(&transition.name),
                    consume.join(", "),
                    produce.join(", ")
                );
                transitions.push(transition_str);
            }
        }

        format!(
            "PLACE\n    {};\n\nMARKING\n    {};\n\n{}",
            places.join(", "),
            markings.join(", "),
            transitions.join("\n\n")
        )
    }

    pub fn analyze_petri_net(&self, petri_net: &PetriNet) -> io::Result<LolaResult> {
        let content = self.convert_to_lola(petri_net);
        let mut file = File::create(self.output_directory.join("pn.lola"))?;
        file.write_all(content.as_bytes())?;

        self.check_deadlock()
    }

    pub fn check_lola_available(&self) -> bool {
        Command::new(&self.lola_path)
            .arg("--version")
            .output()
            .is_ok()
    }

    pub fn check_deadlock(&self) -> io::Result<LolaResult> {
        let output = Command::new(&self.lola_path)
            .arg(&self.output_directory.join("pn.lola"))
            .arg("--formula=EF DEADLOCK")
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = LolaResult {
            has_deadlock: false,
            execution_time: 0.0,
            error: None,
            deadlock_trace: None,
            formula_result: None,
        };

        if !output.status.success() {
            log::error!("LoLA execution failed: {}", stderr);
            result.error = Some(stderr.to_string());
            return Ok(result);
        }

        let output_text = if stderr.is_empty() { stdout } else { stderr };

        for line in output_text.lines() {
            if line.contains("result: yes") || line.contains("The net has deadlock") {
                result.has_deadlock = true;
            } else if line.contains("execution time:") {
                if let Some(time_str) = line.split(':').nth(1) {
                    if let Ok(time) = time_str.trim().parse::<f64>() {
                        result.execution_time = time;
                    }
                }
            } else if line.contains("witness path:") {
                result.deadlock_trace = Some(Vec::new());
            } else if let Some(trace) = &mut result.deadlock_trace {
                trace.push(line.trim().to_string());
            }
        }

        result.formula_result = Some(output_text.to_string());

        Ok(result)
    }

    pub fn check_formula(&self, formula: &str) -> io::Result<LolaResult> {
        let output = Command::new(&self.lola_path)
            .arg(&self.net_file)
            .arg(format!("--formula={}", formula))
            .output()?;

        todo!("Implement custom formula checking")
    }

    pub fn generate_deadlock_report(&self, petri_net: &PetriNet) -> io::Result<DeadlockReport> {
        if !self.check_lola_available() {
            log::error!("LoLA is not available");
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "LoLA is not available",
            ));
        }

        let start_time = std::time::Instant::now();
        let lola_result = self.analyze_petri_net(petri_net)?;
        let analysis_time = start_time.elapsed();

        let mut report = DeadlockReport::new("LoLA".to_string());
        report.analysis_time = analysis_time;
        report.has_deadlock = lola_result.has_deadlock;

        if let Some(trace) = lola_result.deadlock_trace {
            report.deadlock_count = 1;
            report.traces.push(DeadlockTrace {
                steps: trace,
                final_state: None,
            });
        }

        if let Some(error) = lola_result.error {
            report.error = Some(error);
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lola_analyzer() {
        let analyzer = LolaAnalyzer::new(
            "lola".to_string(),
            "tests/mutex.lola".to_string(),
            PathBuf::new(),
        );

        if analyzer.check_lola_available() {
            match analyzer.check_deadlock() {
                Ok(result) => {
                    println!("死锁检测结果: {}", result.has_deadlock);
                    if let Some(trace) = result.deadlock_trace {
                        println!("死锁路径: {:?}", trace);
                    }
                }
                Err(e) => println!("执行错误: {}", e),
            }
        } else {
            println!("LoLA 工具不可用");
        }
    }
}
