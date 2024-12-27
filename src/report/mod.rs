use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockState {
    pub state_id: String,
    pub marking: Vec<(String, usize)>, // (place_name, tokens)
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockTrace {
    pub steps: Vec<String>,
    pub final_state: Option<DeadlockState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockReport {
    pub tool_name: String,                        // 使用的分析工具名称
    pub has_deadlock: bool,                       // 是否存在死锁
    pub deadlock_count: usize,                    // 死锁数量
    pub deadlock_states: Vec<DeadlockState>,      // 死锁状态列表
    pub traces: Vec<DeadlockTrace>,               // 到达死锁的路径
    pub analysis_time: Duration,                  // 分析耗时
    pub state_space_info: Option<StateSpaceInfo>, // 状态空间信息
    pub error: Option<String>,                    // 错误信息
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

            if !self.traces.is_empty() {
                writeln!(f, "\n死锁路径:")?;
                for (i, trace) in self.traces.iter().enumerate() {
                    writeln!(f, "\n路径 #{}", i + 1)?;
                    for (step_num, step) in trace.steps.iter().enumerate() {
                        writeln!(f, "  步骤 {}: {}", step_num + 1, step)?;
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

        // 可选：同时保存JSON格式
        let json_path = format!("{}.json", path);
        std::fs::write(
            json_path,
            serde_json::to_string_pretty(self).unwrap().as_bytes(),
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicOperation {
    pub operation_type: String, // "load" 或 "store"
    pub ordering: String,       // 内存序
    pub variable: String,       // 变量标识
    pub location: String,       // 源代码位置
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicViolation {
    pub violation_type: String, // 违背类型 (e.g. "unsynchronized_path", "concurrent_relaxed")
    pub operations: Vec<AtomicOperation>, // 相关的原子操作
    pub state: Option<Vec<(usize, usize)>>, // 发生违背的状态
    pub path: Option<Vec<String>>, // 违背路径
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicReport {
    pub tool_name: String,
    pub has_violation: bool,
    pub violation_count: usize,
    pub violations: Vec<AtomicViolation>,
    pub analysis_time: Duration,
    pub error: Option<String>,
}

impl fmt::Display for AtomicReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "���子性违背分析报告")?;
        writeln!(f, "分析工具: {}", self.tool_name)?;
        writeln!(f, "分析时间: {:?}", self.analysis_time)?;
        writeln!(f, "是否存在违背: {}", self.has_violation)?;

        if self.has_violation {
            writeln!(f, "\n发现 {} 个原子性违背:", self.violation_count)?;
            for (i, violation) in self.violations.iter().enumerate() {
                writeln!(f, "\n违背 #{}", i + 1)?;
                writeln!(f, "违背类型: {}", violation.violation_type)?;
                writeln!(f, "相关操作:")?;
                for op in &violation.operations {
                    writeln!(
                        f,
                        "  - {} ({}) on {} at {}",
                        op.operation_type, op.ordering, op.variable, op.location
                    )?;
                }

                if let Some(state) = &violation.state {
                    writeln!(f, "违背状态: {:?}", state)?;
                }

                if let Some(path) = &violation.path {
                    writeln!(f, "违背路径:")?;
                    for (step_num, step) in path.iter().enumerate() {
                        writeln!(f, "  步骤 {}: {}", step_num + 1, step)?;
                    }
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

        // 同时保存JSON格式
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
    pub operation_type: String,     // "read" 或 "write"
    pub thread_name: String,        // 线程标识
    pub variable: String,           // 变量标识
    pub location: String,           // 源代码位置
    pub basic_block: Option<usize>, // 基本块信息
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceCondition {
    pub operations: Vec<RaceOperation>,     // 相关的操作
    pub variable_info: VariableInfo,        // 变量信息
    pub state: Option<Vec<(usize, usize)>>, // 发生竞争的状态
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
                writeln!(f, "  名称: {}", race.variable_info.name)?;
                writeln!(f, "  类型: {}", race.variable_info.data_type)?;
                writeln!(f, "  作用域: {}", race.variable_info.function_scope)?;

                writeln!(f, "\n相关操作:")?;
                for op in &race.operations {
                    writeln!(
                        f,
                        "  - {} by {} at {}",
                        op.operation_type, op.thread_name, op.location
                    )?;
                    if let Some(bb) = op.basic_block {
                        writeln!(f, "    (Basic Block: {})", bb)?;
                    }
                }

                if let Some(state) = &race.state {
                    writeln!(f, "\n竞争状态: {:?}", state)?;
                }
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

        // 同时保存JSON格式
        let json_path = format!("{}.json", path);
        std::fs::write(
            json_path,
            serde_json::to_string_pretty(self).unwrap().as_bytes(),
        )?;

        Ok(())
    }
}
