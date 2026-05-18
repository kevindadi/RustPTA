use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const REPORT_WIDTH: usize = 64;

fn write_banner(f: &mut fmt::Formatter<'_>, title: &str) -> fmt::Result {
    writeln!(
        f,
        "{:=^width$}",
        format!(" {} ", title),
        width = REPORT_WIDTH
    )
}

fn write_section(f: &mut fmt::Formatter<'_>, title: &str) -> fmt::Result {
    writeln!(
        f,
        "\n{:-^width$}",
        format!(" {} ", title),
        width = REPORT_WIDTH
    )
}

fn bool_text(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
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
        write_banner(f, "Deadlock analysis report")?;
        writeln!(f, "{:<16}: {}", "Tool", self.tool_name)?;
        writeln!(
            f,
            "{:<16}: {}",
            "Analysis time",
            format_duration(self.analysis_time)
        )?;
        writeln!(
            f,
            "{:<16}: {}",
            "Deadlock present",
            bool_text(self.has_deadlock)
        )?;

        if self.has_deadlock {
            write_section(f, "Deadlock details")?;
            writeln!(f, "Found {} deadlock marking(s).", self.deadlock_count)?;
            for (i, state) in self.deadlock_states.iter().enumerate() {
                writeln!(f, "\n  [{}] State id : {}", i + 1, state.state_id)?;
                writeln!(f, "      Description : {}", state.description)?;
                if !state.marking.is_empty() {
                    writeln!(f, "      Marking snapshot :")?;
                    for (place, tokens) in &state.marking {
                        writeln!(f, "        - {:<24} {}", place, tokens)?;
                    }
                }
            }
        }

        if let Some(space_info) = &self.state_space_info {
            write_section(f, "State space")?;
            writeln!(f, "{:<16}: {}", "Total states", space_info.total_states)?;
            writeln!(f, "{:<16}: {}", "Total transitions", space_info.total_transitions)?;
            writeln!(f, "{:<16}: {}", "Reachable states", space_info.reachable_states)?;
        }

        if let Some(error) = &self.error {
            write_section(f, "Errors")?;
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
        write_banner(f, "Atomicity violation report")?;
        writeln!(f, "{:<16}: {}", "Tool", self.tool_name)?;
        writeln!(
            f,
            "{:<16}: {}",
            "Analysis time",
            format_duration(self.analysis_time)
        )?;
        writeln!(
            f,
            "{:<16}: {}",
            "Violation present",
            bool_text(self.has_violation)
        )?;

        if self.has_violation {
            write_section(f, "Violation details")?;
            writeln!(
                f,
                "Found {} atomicity violation pattern(s).",
                self.violation_count
            )?;
            for (i, pattern) in self.violations.iter().enumerate() {
                writeln!(
                    f,
                    "\n  [{}] Load op    : {} @ {} ({})",
                    i + 1,
                    pattern.load_op.variable,
                    pattern.load_op.location,
                    pattern.load_op.ordering
                )?;
                writeln!(f, "      Conflicting stores :")?;
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
            write_section(f, "Errors")?;
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
        write_banner(f, "Data race report")?;
        writeln!(f, "{:<16}: {}", "Tool", self.tool_name)?;
        writeln!(
            f,
            "{:<16}: {}",
            "Analysis time",
            format_duration(self.analysis_time)
        )?;
        writeln!(f, "{:<16}: {}", "Race present", bool_text(self.has_race))?;

        if self.has_race {
            write_section(f, "Race details")?;
            writeln!(f, "Found {} data race(s).", self.race_count)?;
            for (i, race) in self.race_conditions.iter().enumerate() {
                writeln!(f, "\n  [{}] Variable : {}", i + 1, race.variable_info)?;
                writeln!(f, "      Operations :")?;
                for op in &race.operations {
                    writeln!(f, "        - {:<6} @ {}", op.operation_type, op.location)?;
                    if let Some(bb) = op.basic_block {
                        writeln!(f, "            Basic block : {}", bb)?;
                    }
                }

                writeln!(f, "      Race marking : {:?}", race.state)?;
            }
        }

        if let Some(error) = &self.error {
            write_section(f, "Errors")?;
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
