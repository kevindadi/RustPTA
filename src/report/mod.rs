use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
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
        writeln!(f, "死锁分析报告")?;
        writeln!(f, "分析工具: {}", self.tool_name)?;
        writeln!(f, "分析时间: {:?}", self.analysis_time)?;
        writeln!(f, "是否存在死锁: {}", self.has_deadlock)?;

        if self.has_deadlock {
            writeln!(f, "\n发现 {} 个死锁状态:", self.deadlock_count)?;
            for (i, state) in self.deadlock_states.iter().enumerate() {
                writeln!(f, "\n死锁 #{}", i + 1)?;
                writeln!(f, "状态ID: {}", state.state_id)?;
                writeln!(f, "描述: {}", state.description)?;
                if !state.marking.is_empty() {
                    writeln!(f, "标识:")?;
                    for (place, tokens) in &state.marking {
                        writeln!(f, "  {}: {}", place, tokens)?;
                    }
                }
            }
        }

        if let Some(space_info) = &self.state_space_info {
            writeln!(f, "\n状态空间信息:")?;
            writeln!(f, "总状态数: {}", space_info.total_states)?;
            writeln!(f, "总转换数: {}", space_info.total_transitions)?;
            writeln!(f, "可达状态数: {}", space_info.reachable_states)?;
        }

        if let Some(error) = &self.error {
            writeln!(f, "\n错误信息: {}", error)?;
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
        writeln!(f, "原子性违背分析报告")?;
        writeln!(f, "分析工具: {}", self.tool_name)?;
        writeln!(f, "分析时间: {:?}", self.analysis_time)?;
        writeln!(f, "是否存在违背: {}", self.has_violation)?;

        if self.has_violation {
            writeln!(f, "\n发现 {} 个原子性违背模式:", self.violation_count)?;
            for (i, pattern) in self.violations.iter().enumerate() {
                writeln!(f, "\n违背模式 #{}:", i + 1)?;
                writeln!(
                    f,
                    "- Load Operation: {} at {} ({})",
                    pattern.load_op.variable, pattern.load_op.location, pattern.load_op.ordering
                )?;
                writeln!(f, "- Conflicting Store Operations:")?;
                for (j, store) in pattern.store_ops.iter().enumerate() {
                    writeln!(
                        f,
                        "  {}. Store at {} ({}) on {}",
                        j + 1,
                        store.location,
                        store.ordering,
                        store.variable
                    )?;
                }
            }
        }

        if let Some(error) = &self.error {
            writeln!(f, "\n错误信息: {}", error)?;
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
        writeln!(f, "数据竞争分析报告")?;
        writeln!(f, "分析工具: {}", self.tool_name)?;
        writeln!(f, "分析时间: {:?}", self.analysis_time)?;
        writeln!(f, "是否存在竞争: {}", self.has_race)?;

        if self.has_race {
            writeln!(f, "\n发现 {} 个数据竞争:", self.race_count)?;
            for (i, race) in self.race_conditions.iter().enumerate() {
                writeln!(f, "\n竞争 #{}", i + 1)?;
                writeln!(f, "变量信息:")?;
                writeln!(f, "  名称: {}", race.variable_info)?;

                writeln!(f, "\n相关操作:")?;
                for op in &race.operations {
                    writeln!(f, "  - {} at {}", op.operation_type, op.location)?;
                    if let Some(bb) = op.basic_block {
                        writeln!(f, "    (Basic Block: {})", bb)?;
                    }
                }

                writeln!(f, "\n竞争状态: {:?}", race.state)?;
            }
        }

        if let Some(error) = &self.error {
            writeln!(f, "\n错误信息: {}", error)?;
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
