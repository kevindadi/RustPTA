use crate::graph::net_structure::PetriNetNode;
use crate::graph::pn::PetriNet;
use crate::report::{DeadlockReport, DeadlockState, StateSpaceInfo};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::{
    fs::File,
    io::{self, Write},
};

use super::normalize_name_for_tina;

#[derive(Debug, Serialize, Deserialize)]
pub struct TinaResult {
    pub has_deadlock: bool,
    pub state_space_size: usize,
    pub transition_count: usize,
    pub error: Option<String>,
    pub deadlock_states: Option<Vec<String>>,
    pub analysis_output: Option<String>,
}

#[derive(Debug)]
pub struct TinaAnalyzer {
    tina_path: String,
    net_file: String,
    pub output_directory: PathBuf,
}

impl TinaAnalyzer {
    pub fn new(tina_path: String, net_file: String, output_directory: PathBuf) -> Self {
        Self {
            tina_path,
            net_file,
            output_directory,
        }
    }

    // .net                    ::= (<trdesc>|<pldesc>|<lbdesc>|<prdesc>|<ntdesc>|<netdesc>)*
    // netdesc                 ::= ’net’ <net>
    // trdesc                  ::= ’tr’ <transition> {":" <label>} {<interval>} {<tinput> -> <toutput>}
    // pldesc                  ::= ’pl’ <place> {":" <label>} {(<marking>)} {<pinput> -> <poutput>}
    // ntdesc                  ::= ’nt’ <note> (’0’|’1’) <annotation>
    // lbdesc                  ::= ’lb’ [<place>|<transition>] <label>
    // prdesc                  ::= ’pr’ (<transition>)+ ("<"|">") (<transition>)+
    // interval                        ::= (’[’|’]’)INT’,’INT(’[’|’]’) | (’[’|’]’)INT’,’w[’
    // tinput                  ::= <place>{<arc>}
    // toutput                 ::= <place>{<normal_arc>}
    // pinput                  ::= <transition>{<normal_arc>}
    // poutput                 ::= <transition>{arc}
    // arc                     ::= <normal_arc> | <test_arc> | <inhibitor_arc> |
    //                             <stopwatch_arc> | <stopwatch-inhibitor_arc>
    // normal_arc              ::= ’*’<weight>
    // test_arc                ::= ’?’<weight>
    // inhibitor_arc           ::= ’?-’<weight>
    // stopwatch_arc           ::= ’!’<weight>
    // stopwatch-inhibitor_arc ::= ’!-’<weight>
    // weight, marking         ::= INT{’K’|’M’|’G’|’T’|’P’|’E’}
    // net, place, transition, label, note, annotation ::= ANAME | ’{’QNAME’}’
    // INT                     ::= unsigned integer
    // ANAME                   ::= alphanumeric name, see Notes below
    // QNAME                   ::= arbitrary name, see Notes below
    pub fn convert_to_tina(&self, petri_net: &PetriNet) -> String {
        let mut output = String::new();
        output.push_str("net PetriNet\n");

        for index in petri_net.net.node_indices() {
            if let PetriNetNode::P(place) = &petri_net.net[index] {
                let tokens = place.tokens.borrow();
                let mut pl_str =
                    format!("pl {} ({})", normalize_name_for_tina(&place.name), tokens);

                let mut inputs = Vec::new();
                let mut outputs = Vec::new();

                for edge in petri_net.net.edges_directed(index, Direction::Incoming) {
                    if let PetriNetNode::T(transition) = &petri_net.net[edge.source()] {
                        inputs.push(format!(
                            "{}*{}",
                            normalize_name_for_tina(&transition.name),
                            edge.weight().label
                        ));
                    }
                }

                for edge in petri_net.net.edges_directed(index, Direction::Outgoing) {
                    if let PetriNetNode::T(transition) = &petri_net.net[edge.target()] {
                        outputs.push(format!(
                            "{}*{}",
                            normalize_name_for_tina(&transition.name),
                            edge.weight().label
                        ));
                    }
                }

                if !inputs.is_empty() || !outputs.is_empty() {
                    pl_str.push_str(" ");
                    pl_str.push_str(&format!("{} -> {}", inputs.join(" "), outputs.join(" ")));
                }

                output.push_str(&format!("{}\n", pl_str));
            }
        }

        for index in petri_net.net.node_indices() {
            if let PetriNetNode::T(transition) = &petri_net.net[index] {
                let mut tr_str = format!("tr {}", normalize_name_for_tina(&transition.name));

                let mut inputs = Vec::new();
                let mut outputs = Vec::new();

                for edge in petri_net.net.edges_directed(index, Direction::Incoming) {
                    if let PetriNetNode::P(place) = &petri_net.net[edge.source()] {
                        inputs.push(format!(
                            "{}*{}",
                            normalize_name_for_tina(&place.name),
                            edge.weight().label
                        ));
                    }
                }

                for edge in petri_net.net.edges_directed(index, Direction::Outgoing) {
                    if let PetriNetNode::P(place) = &petri_net.net[edge.target()] {
                        outputs.push(format!(
                            "{}*{}",
                            normalize_name_for_tina(&place.name),
                            edge.weight().label
                        ));
                    }
                }

                tr_str.push_str(&format!(" {} -> {}", inputs.join(" "), outputs.join(" ")));

                output.push_str(&format!("{}\n", tr_str));
            }
        }

        output
    }

    pub fn analyze_petri_net(&self, petri_net: &PetriNet) -> io::Result<TinaResult> {
        // 转换为 Tina 格式并保存到文件
        let content = self.convert_to_tina(petri_net);
        let net_file = self.output_directory.join("tina.net");
        let mut file = File::create(&net_file)?;
        file.write_all(content.as_bytes())?;

        self.check_deadlock(&net_file)
    }

    /// 检查 Tina 是否可用
    pub fn check_tina_available(&self) -> bool {
        Command::new("/usr/local/tina/bin/tina")
            .arg("-h")
            .output()
            .is_ok()
    }

    /// 检查死锁
    pub fn check_deadlock(&self, net_file: &PathBuf) -> io::Result<TinaResult> {
        let output = Command::new("/usr/local/tina/bin/tina")
            .arg(net_file)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let output_text = if stdout.is_empty() { stderr } else { stdout };

        // 保存完整输出到文件
        let output_file = self.output_directory.join("tina_analysis.txt");
        std::fs::write(&output_file, output_text.as_bytes())?;

        let mut result = TinaResult {
            has_deadlock: false,
            state_space_size: 0,
            transition_count: 0,
            error: None,
            deadlock_states: None,
            analysis_output: None,
        };

        // 解析输出
        let mut in_liveness_section = false;
        let mut liveness_info = String::new();
        let mut analysis_summary = String::new();

        for line in output_text.lines() {
            if line.contains("LIVENESS ANALYSIS") {
                in_liveness_section = true;
                liveness_info.push_str(line);
                liveness_info.push('\n');
            } else if line.contains("ANALYSIS COMPLETED") {
                in_liveness_section = false;
                for summary_line in line.lines() {
                    if summary_line.starts_with('#') {
                        analysis_summary.push_str(summary_line);
                        analysis_summary.push('\n');
                    }
                }
            } else if in_liveness_section {
                liveness_info.push_str(line);
                liveness_info.push('\n');
            }

            // 解析状态空间信息
            if line.contains("states") && line.contains("transitions") {
                if let Some(states) = line
                    .split_whitespace()
                    .find(|&s| s.chars().all(|c| c.is_digit(10)))
                {
                    result.state_space_size = states.parse().unwrap_or(0);
                }
                if let Some(trans) = line
                    .split_whitespace()
                    .skip(1)
                    .find(|&s| s.chars().all(|c| c.is_digit(10)))
                {
                    result.transition_count = trans.parse().unwrap_or(0);
                }
            }

            // 检查死锁状态
            if line.contains("dead") {
                result.has_deadlock = true;
                if result.deadlock_states.is_none() {
                    result.deadlock_states = Some(Vec::new());
                }
                if let Some(states) = &mut result.deadlock_states {
                    states.push(line.to_string());
                }
            }
        }

        // 只保存活性分析和总结信息
        result.analysis_output = Some(format!("{}\n{}", liveness_info, analysis_summary));

        Ok(result)
    }

    pub fn get_analysis_info(&self) -> io::Result<String> {
        if let Ok(result) = self.check_deadlock(&self.output_directory.join("tina.net")) {
            if let Some(output) = result.analysis_output {
                Ok(output)
            } else {
                Ok("分析执行失败: 没有输出".to_string())
            }
        } else {
            Ok("分析执行失败".to_string())
        }
    }

    pub fn generate_deadlock_report(&self, petri_net: &PetriNet) -> io::Result<DeadlockReport> {
        if !self.check_tina_available() {
            log::error!("Tina is not available");
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Tina is not available",
            ));
        }

        let start_time = std::time::Instant::now();
        let tina_result = self.analyze_petri_net(petri_net)?;
        let analysis_time = start_time.elapsed();

        let mut report = DeadlockReport::new("TINA".to_string());
        report.analysis_time = analysis_time;
        report.has_deadlock = tina_result.has_deadlock;

        if let Some(states) = tina_result.deadlock_states {
            report.deadlock_count = states.len();
            for state in states {
                report.deadlock_states.push(DeadlockState {
                    state_id: "unknown".to_string(),
                    marking: Vec::new(),
                    description: state,
                });
            }
        }

        report.state_space_info = Some(StateSpaceInfo {
            total_states: tina_result.state_space_size,
            total_transitions: tina_result.transition_count,
            reachable_states: tina_result.state_space_size,
        });

        if let Some(error) = tina_result.error {
            report.error = Some(error);
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tina_analyzer() {
        let analyzer = TinaAnalyzer::new(
            "tina".to_string(),
            "/home/kevin/RustPTA/tests/ifip.net".to_string(),
            PathBuf::from("./tmp"),
        );

        if analyzer.check_tina_available() {
            // 测试代码
        } else {
            println!("Tina 工具不可用");
        }
    }
}
