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
