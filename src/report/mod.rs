use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const REPORT_WIDTH: usize = 64;

fn write_banner(f: &mut fmt::Formatter<'_>, title: &str) -> fmt::Result {
    writeln!(f, "{:=^width$}", format!(" {} ", title), width = REPORT_WIDTH)
}

fn write_section(f: &mut fmt::Formatter<'_>, title: &str) -> fmt::Result {
    writeln!(f, "\n{:-^width$}", format!(" {} ", title), width = REPORT_WIDTH)
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "是"
    } else {
        "否"
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.is_zero() {
        "0.000 s".to_string()
    } else {
        format!("{:.3} s", duration.as_secs_f64())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockState {
    pub state_id: String,
    pub marking: Vec<(String, u8)>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockTrace {
    pub steps: Vec<String>,
    pub final_state: Option<DeadlockState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockReport {
    pub tool_name: String,
    pub has_deadlock: bool,
    pub deadlock_count: usize,
    pub deadlock_states: Vec<DeadlockState>,
    pub traces: Vec<DeadlockTrace>,
    pub analysis_time: Duration,
    pub state_space_info: Option<StateSpaceInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSpaceInfo {
    pub total_states: usize,
    pub total_transitions: usize,
    pub reachable_states: usize,
}

impl fmt::Display for DeadlockReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_banner(f, "死锁分析报告")?;
        writeln!(f, "{:<16}: {}", "分析工具", self.tool_name)?;
        writeln!(f, "{:<16}: {}", "分析耗时", format_duration(self.analysis_time))?;
        writeln!(f, "{:<16}: {}", "是否存在死锁", bool_text(self.has_deadlock))?;

        if self.has_deadlock {
            write_section(f, "死锁详情")?;
            writeln!(f, "共发现 {} 个死锁状态.", self.deadlock_count)?;
            for (i, state) in self.deadlock_states.iter().enumerate() {
                writeln!(f, "\n  [{}] 状态ID   : {}", i + 1, state.state_id)?;
                writeln!(f, "      描述     : {}", state.description)?;
                if !state.marking.is_empty() {
                    writeln!(f, "      标识快照 :")?;
                    for (place, tokens) in &state.marking {
                        writeln!(f, "        - {:<24} {}", place, tokens)?;
                    }
                }
            }
        }

        if let Some(space_info) = &self.state_space_info {
            write_section(f, "状态空间")?;
            writeln!(f, "{:<16}: {}", "总状态数", space_info.total_states)?;
            writeln!(f, "{:<16}: {}", "总转换数", space_info.total_transitions)?;
            writeln!(f, "{:<16}: {}", "可达状态数", space_info.reachable_states)?;
        }

        if let Some(error) = &self.error {
            write_section(f, "错误信息")?;
            writeln!(f, "{}", error)?;
        }

        Ok(())
    }
}

impl DeadlockReport {
    pub fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            has_deadlock: false,
            deadlock_count: 0,
            deadlock_states: Vec::new(),
            traces: Vec::new(),
            analysis_time: Duration::default(),
            state_space_info: None,
            error: None,
        }
    }

    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(path)?;
        writeln!(file, "{}", self)?;

        let json_path = format!("{}.json", path);
        std::fs::write(
            json_path,
            serde_json::to_string_pretty(self).unwrap().as_bytes(),
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AtomicOperation {
    pub operation_type: String,
    pub ordering: String,
    pub variable: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicViolation {
    pub pattern: ViolationPattern,
    pub states: Vec<(usize, u8)>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ViolationPattern {
    pub load_op: AtomicOperation,
    pub store_ops: Vec<AtomicOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicReport {
    pub tool_name: String,
    pub has_violation: bool,
    pub violation_count: usize,
    pub violations: Vec<ViolationPattern>,
    pub analysis_time: Duration,
    pub error: Option<String>,
}

impl fmt::Display for AtomicReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_banner(f, "原子性违背分析报告")?;
        writeln!(f, "{:<16}: {}", "分析工具", self.tool_name)?;
        writeln!(f, "{:<16}: {}", "分析耗时", format_duration(self.analysis_time))?;
        writeln!(f, "{:<16}: {}", "是否存在违背", bool_text(self.has_violation))?;

        if self.has_violation {
            write_section(f, "违背详情")?;
            writeln!(f, "共发现 {} 个原子性违背模式.", self.violation_count)?;
            for (i, pattern) in self.violations.iter().enumerate() {
                writeln!(f, "\n  [{}] Load 操作  : {} @ {} ({})",
                    i + 1,
                    pattern.load_op.variable,
                    pattern.load_op.location,
                    pattern.load_op.ordering
                )?;
                writeln!(f, "      Store 冲突 :")?;
                for (j, store) in pattern.store_ops.iter().enumerate() {
                    writeln!(
                        f,
                        "        {}. {} @ {} ({})",
                        j + 1,
                        store.variable,
                        store.ordering,
                        store.location
                    )?;
                }
            }
        }

        if let Some(error) = &self.error {
            write_section(f, "错误信息")?;
            writeln!(f, "{}", error)?;
        }

        Ok(())
    }
}

impl AtomicReport {
    pub fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            has_violation: false,
            violation_count: 0,
            violations: Vec::new(),
            analysis_time: Duration::default(),
            error: None,
        }
    }

    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(path)?;
        writeln!(file, "{}", self)?;

        let json_path = format!("{}.json", path);
        std::fs::write(
            json_path,
            serde_json::to_string_pretty(self).unwrap().as_bytes(),
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceOperation {
    pub operation_type: String,
    pub variable: String,
    pub location: String,
    pub basic_block: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceCondition {
    pub operations: Vec<RaceOperation>,
    pub variable_info: String,
    pub state: Vec<(usize, u8)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub data_type: String,
    pub function_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceReport {
    pub tool_name: String,
    pub has_race: bool,
    pub race_count: usize,
    pub race_conditions: Vec<RaceCondition>,
    pub analysis_time: Duration,
    pub error: Option<String>,
}

impl fmt::Display for RaceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_banner(f, "数据竞争分析报告")?;
        writeln!(f, "{:<16}: {}", "分析工具", self.tool_name)?;
        writeln!(f, "{:<16}: {}", "分析耗时", format_duration(self.analysis_time))?;
        writeln!(f, "{:<16}: {}", "是否存在竞争", bool_text(self.has_race))?;

        if self.has_race {
            write_section(f, "竞争详情")?;
            writeln!(f, "共发现 {} 个数据竞争.", self.race_count)?;
            for (i, race) in self.race_conditions.iter().enumerate() {
                writeln!(f, "\n  [{}] 变量信息 : {}", i + 1, race.variable_info)?;
                writeln!(f, "      相关操作 :")?;
                for op in &race.operations {
                    writeln!(
                        f,
                        "        - {:<6} @ {}",
                        op.operation_type,
                        op.location
                    )?;
                    if let Some(bb) = op.basic_block {
                        writeln!(f, "            基本块 : {}", bb)?;
                    }
                }

                writeln!(f, "      竞争状态 : {:?}", race.state)?;
            }
        }

        if let Some(error) = &self.error {
            write_section(f, "错误信息")?;
            writeln!(f, "{}", error)?;
        }

        Ok(())
    }
}

impl RaceReport {
    pub fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            has_race: false,
            race_count: 0,
            race_conditions: Vec::new(),
            analysis_time: Duration::default(),
            error: None,
        }
    }

    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(path)?;
        writeln!(file, "{}", self)?;

        let json_path = format!("{}.json", path);
        std::fs::write(
            json_path,
            serde_json::to_string_pretty(self).unwrap().as_bytes(),
        )?;

        Ok(())
    }
}
