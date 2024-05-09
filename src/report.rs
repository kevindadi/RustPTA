use log::debug;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{collections::HashMap, io::Write};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionDetail {
    action_type: String,
    size: usize,
    address: String,
    thread_name: String,
    file_span: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRaceReport {
    pub filename: String,              // 发生数据竞争的文件名
    pub line_number: usize,            // 发生数据竞争的行号         // 发生数据竞争的变量名
    pub current_action: ActionDetail,  // 当前操作
    pub previous_action: ActionDetail, // 之前操作
    pub message: String,               // 详细描述或附加消息
}

impl fmt::Display for ActionDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} in {}",
            self.thread_name, self.action_type, self.file_span,
        )
    }
}

impl DataRaceReport {
    /// 创建一个新的数据竞争报告
    fn new(
        filename: String,
        message: String,
        current_action: ActionDetail,
        previous_action: ActionDetail,
    ) -> Self {
        DataRaceReport {
            filename,
            line_number: 0usize,
            current_action,
            previous_action,
            message,
        }
    }

    /// 将报告保存到文件中
    pub fn save_to_file(&self, file_path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(file_path)?;
        // let mut writer = std::io::BufWriter::new(file);
        // let report = serde_json::to_string_pretty(&self)?;
        // writer.write_all(report.as_bytes())
        writeln!(file, "{}", self)
    }
}

impl fmt::Display for DataRaceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Data Race Report:\nLocation: {}\n{}--->{}",
            self.message, self.previous_action, self.current_action,
        )
    }
}

pub fn parse_thread_sanitizer_report(capture: &str) -> Vec<DataRaceReport> {
    let mut reports = Vec::new();
    let action_regex =
        Regex::new(r"(Read|Write) of size (\d+) at (0x[0-9a-f]+) by (thread T\d+|main thread)")
            .unwrap();
    let previous_action_regex = Regex::new(
        r"(?i)^\s*Previous\s+(Read|Write)\s+of\s+size\s+(\d+)\s+at\s+(0x[0-9a-f]+)\s+by\s+(thread T\d+|main thread)",
    )
    .unwrap();

    //let summary_regex = Regex::new(r"SUMMARY: ThreadSanitizer: data race (.*)").unwrap();
    let summary_regex = Regex::new(
        r"(?P<file>[^:\s]+\.rs):(?P<line>\d+) in (?P<function>[^s:]+?::[^\s:]+)(?:::[^\s:]+)",
    )
    .unwrap();
    let span_regex = Regex::new(r"(\w+\.rs):(\d+)").unwrap();

    for cap in capture.split("WARNING: ThreadSanitizer: data race").skip(1) {
        let mut ready_to_build_report = false;
        let mut summary = String::new();
        let mut current_action = ActionDetail::default();
        let mut previous_action = ActionDetail::default();

        let mut lines_map: HashMap<usize, &str> = HashMap::new();
        for (index, line) in cap.lines().enumerate() {
            lines_map.insert(index, line);
        }

        for (index, line) in cap.lines().enumerate() {
            if let Some(caps) = summary_regex.captures(line) {
                summary = caps.name("file").unwrap().as_str().to_string()
                    + ":"
                    + caps.name("line").unwrap().as_str()
                    + "->"
                    + caps.name("function").unwrap().as_str();
                debug!("{}", summary);
                ready_to_build_report = true;
            }
            if let Some(caps) = action_regex.captures(line) {
                debug!("find current action");
                current_action.action_type = caps.get(1).unwrap().as_str().to_string();
                current_action.size = caps.get(2).unwrap().as_str().to_string().parse().unwrap();
                current_action.address = caps.get(3).unwrap().as_str().to_string();
                current_action.thread_name = caps.get(4).unwrap().as_str().to_string();
                if let Some(caps) = span_regex.captures(lines_map.get(&(index + 1)).unwrap()) {
                    current_action.file_span = caps.get(1).unwrap().as_str().to_string()
                        + ":"
                        + caps.get(2).unwrap().as_str();
                }
                debug!("{:?}", current_action);
            }
            if let Some(caps) = previous_action_regex.captures(line) {
                debug!("find previous action");
                previous_action.action_type = caps[1].to_string();
                previous_action.size = caps[2].parse().unwrap();
                previous_action.address = caps[3].to_string();
                previous_action.thread_name = caps[4].to_string();
                if let Some(caps) = span_regex.captures(lines_map.get(&(index + 1)).unwrap()) {
                    previous_action.file_span = caps.get(1).unwrap().as_str().to_string()
                        + ":"
                        + caps.get(2).unwrap().as_str();
                }
                log::info!("{:?}", previous_action);
            }
        }
        if ready_to_build_report && !summary.is_empty() {
            let report = DataRaceReport::new(
                "main.rs".to_string(),
                summary.clone(),
                current_action.clone(),
                previous_action.clone(),
            );
            reports.push(report);
        }
    }
    reports
}

#[cfg(test)]
pub mod test_s {
    use super::parse_thread_sanitizer_report;

    #[test]
    pub fn thread_sanitizer_test() {
        let output = "==================
        WARNING: ThreadSanitizer: data race (pid=21236)
          Write of size 4 at 0x00010bfb6138 by thread T1:
            #0 data_race::main::_$u7b$$u7b$closure$u7d$$u7d$::h46c11a1d2f4f7a3c main.rs:6 (data_race:x86_64+0x10000489c)
            #1 std::sys_common::backtrace::__rust_begin_short_backtrace::hb112272e9bb6db2d backtrace.rs:154 (data_race:x86_64+0x100006ae5)
            #2 std::thread::Builder::spawn_unchecked_::_$u7b$$u7b$closure$u7d$$u7d$::_$u7b$$u7b$closure$u7d$$u7d$::hc2a42f0363bd3455 mod.rs:529 (data_race:x86_64+0x100006905)
            #3 _$LT$core..panic..unwind_safe..AssertUnwindSafe$LT$F$GT$$u20$as$u20$core..ops..function..FnOnce$LT$$LP$$RP$$GT$$GT$::call_once::he1d66c460f9b4f8c unwind_safe.rs:271 (data_race:x86_64+0x100006a45)
            #4 std::panicking::try::do_call::h07c3dec6665898ef panicking.rs:526 (data_race:x86_64+0x100009245)
            #5 __rust_try <null>:211077593 (data_race:x86_64+0x100009456)
            #6 std::panicking::try::h0b5d81ac35d90170 panicking.rs:490 (data_race:x86_64+0x1000090cb)
            #7 std::thread::Builder::spawn_unchecked_::_$u7b$$u7b$closure$u7d$$u7d$::h8733b586313c5331 mod.rs:528 (data_race:x86_64+0x100006765)
            #8 core::ops::function::FnOnce::call_once$u7b$$u7b$vtable.shim$u7d$$u7d$::haf1116a148e6401e function.rs:250 (data_race:x86_64+0x1000015f1)
            #9 std::sys::unix::thread::Thread::new::thread_start::hbd6b3c940ffebb42 <null>:211077593 (data_race:x86_64+0x10002cad8)
        
          Previous write of size 4 at 0x00010bfb6138 by main thread:
            #0 data_race::main::h9aaa3d02cd49da53 main.rs:8 (data_race:x86_64+0x1000047a7)
            #1 core::ops::function::FnOnce::call_once::h7fc327ab93fc3eeb function.rs:250 (data_race:x86_64+0x10000177e)
            #2 std::sys_common::backtrace::__rust_begin_short_backtrace::hacae4f21585e5c7d backtrace.rs:154 (data_race:x86_64+0x100006aa1)
            #3 std::rt::lang_start::_$u7b$$u7b$closure$u7d$$u7d$::hdcfea15eec5df62f rt.rs:166 (data_race:x86_64+0x100006c0d)
            #4 std::rt::lang_start_internal::he25611ff05acf1eb <null>:211077593 (data_race:x86_64+0x1000248f9)
            #5 main <null>:211077593 (data_race:x86_64+0x10000483f)
        
          Location is global 'data_race::ANSWER::h46fc85e338847902' at 0x00010bfb6138 (data_race+0x100055138)
        
          Thread T1 (tid=5759124, running) created by main thread at:
            #0 pthread_create <null>:211077593 (librustc-nightly_rt.tsan.dylib:x86_64h+0x9aef)
            #1 std::sys::unix::thread::Thread::new::h249fa67ab56bb63c <null>:211077593 (data_race:x86_64+0x10002c93f)
            #2 std::thread::Builder::spawn_unchecked::h3f024ac906f0489c mod.rs:457 (data_race:x86_64+0x100005b7c)
            #3 std::thread::spawn::h9dc8f5993b4aa544 mod.rs:686 (data_race:x86_64+0x100005aee)
            #4 data_race::main::h9aaa3d02cd49da53 main.rs:6 (data_race:x86_64+0x100004799)
            #5 core::ops::function::FnOnce::call_once::h7fc327ab93fc3eeb function.rs:250 (data_race:x86_64+0x10000177e)
            #6 std::sys_common::backtrace::__rust_begin_short_backtrace::hacae4f21585e5c7d backtrace.rs:154 (data_race:x86_64+0x100006aa1)
            #7 std::rt::lang_start::_$u7b$$u7b$closure$u7d$$u7d$::hdcfea15eec5df62f rt.rs:166 (data_race:x86_64+0x100006c0d)
            #8 std::rt::lang_start_internal::he25611ff05acf1eb <null>:211077593 (data_race:x86_64+0x1000248f9)
            #9 main <null>:211077593 (data_race:x86_64+0x10000483f)
        
        SUMMARY: ThreadSanitizer: data race main.rs:6 in data_race::main::_$u7b$$u7b$closure$u7d$$u7d$::h46c11a1d2f4f7a3c
        ==================";

        let actions = parse_thread_sanitizer_report(output);

        for action in actions {
            let _ = action.save_to_file("report.txt");
        }
    }
}
