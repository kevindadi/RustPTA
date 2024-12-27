use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

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
