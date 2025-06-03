use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockState {
    pub state_id: String,
    pub marking: Vec<(String, u8)>, // (place_name, tokens)
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockTrace {
    pub steps: Vec<String>,
    pub final_state: Option<DeadlockState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlockReport {
    pub tool_name: String,                        // Analysis tool name used
    pub has_deadlock: bool,                       // Whether deadlock exists
    pub deadlock_count: usize,                    // Number of deadlocks
    pub deadlock_states: Vec<DeadlockState>,      // List of deadlock states
    pub traces: Vec<DeadlockTrace>,               // Paths to deadlock
    pub analysis_time: Duration,                  // Analysis time cost
    pub state_space_info: Option<StateSpaceInfo>, // State space information
    pub error: Option<String>,                    // Error information
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSpaceInfo {
    pub total_states: usize,
    pub total_transitions: usize,
    pub reachable_states: usize,
}

impl fmt::Display for DeadlockReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Deadlock Analysis Report")?;
        writeln!(f, "Analysis Tool: {}", self.tool_name)?;
        writeln!(f, "Analysis Time: {:?}", self.analysis_time)?;
        writeln!(f, "Deadlock Found: {}", self.has_deadlock)?;

        if self.has_deadlock {
            writeln!(f, "\nFound {} deadlock states:", self.deadlock_count)?;
            for (i, state) in self.deadlock_states.iter().enumerate() {
                writeln!(f, "\nDeadlock #{}", i + 1)?;
                writeln!(f, "State ID: {}", state.state_id)?;
                writeln!(f, "Description: {}", state.description)?;
                if !state.marking.is_empty() {
                    writeln!(f, "Tokens:")?;
                    for (place, tokens) in &state.marking {
                        writeln!(f, "  {}: {}", place, tokens)?;
                    }
                }
            }

            // if !self.traces.is_empty() {
            //     writeln!(f, "\nDeadlock paths:")?;
            //     for (i, trace) in self.traces.iter().enumerate() {
            //         writeln!(f, "\nPath #{}", i + 1)?;
            //         for (step_num, step) in trace.steps.iter().enumerate() {
            //             writeln!(f, "  Step {}: {}", step_num + 1, step)?;
            //         }
            //     }
            // }
        }

        if let Some(space_info) = &self.state_space_info {
            writeln!(f, "\nState Space Information:")?;
            writeln!(f, "Total States: {}", space_info.total_states)?;
            writeln!(f, "Total Transitions: {}", space_info.total_transitions)?;
            writeln!(f, "Reachable States: {}", space_info.reachable_states)?;
        }

        if let Some(error) = &self.error {
            writeln!(f, "\nError Information: {}", error)?;
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
        writeln!(f, "Atomicity Violation Analysis Report")?;
        writeln!(f, "Analysis Tool: {}", self.tool_name)?;
        writeln!(f, "Analysis Time: {:?}", self.analysis_time)?;
        writeln!(f, "Violation Found: {}", self.has_violation)?;

        if self.has_violation {
            writeln!(f, "\nFound {} atomicity violation patterns:", self.violation_count)?;
            for (i, pattern) in self.violations.iter().enumerate() {
                writeln!(f, "\nViolation Pattern #{}:", i + 1)?;
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
            writeln!(f, "\nError Information: {}", error)?;
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
    pub operation_type: String,     // Operation type (Read/Write)
    pub variable: String,           // Variable identifier
    pub location: String,           // Source code location
    pub basic_block: Option<usize>, // Basic block information
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceCondition {
    pub operations: Vec<RaceOperation>, // Related operations
    pub variable_info: String,          // Variable information
    pub state: Vec<(usize, u8)>,        // State where race occurs
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
        writeln!(f, "Data Race Analysis Report")?;
        writeln!(f, "Analysis Tool: {}", self.tool_name)?;
        writeln!(f, "Analysis Time: {:?}", self.analysis_time)?;
        writeln!(f, "Race Found: {}", self.has_race)?;

        if self.has_race {
            writeln!(f, "\nFound {} data races:", self.race_count)?;
            for (i, race) in self.race_conditions.iter().enumerate() {
                writeln!(f, "\nRace #{}", i + 1)?;
                writeln!(f, "Variable Information:")?;
                writeln!(f, "  Name: {}", race.variable_info)?;
                // writeln!(f, "  类型: {}", race.variable_info.data_type)?;
                // writeln!(f, "  作用域: {}", race.variable_info.function_scope)?;

                writeln!(f, "\nRelated Operations:")?;
                for op in &race.operations {
                    writeln!(f, "  - {} at {}", op.operation_type, op.location)?;
                    if let Some(bb) = op.basic_block {
                        writeln!(f, "    (Basic Block: {})", bb)?;
                    }
                }

                writeln!(f, "\nRace State: {:?}", race.state)?;
            }
        }

        if let Some(error) = &self.error {
            writeln!(f, "\nError Information: {}", error)?;
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
