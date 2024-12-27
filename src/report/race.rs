use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

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
