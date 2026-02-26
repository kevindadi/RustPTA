extern crate rustc_driver;
extern crate rustc_hir;

use crate::analysis::reachability::{StateGraph, StateGraphConfig};
#[cfg(feature = "atomic-violation")]
use crate::detect::atomic_violation_detector::{
    Witness, detect_atomicity_violations, marking_from_places, print_witnesses,
};
#[cfg(not(feature = "atomic-violation"))]
use crate::detect::atomicity_violation::AtomicityViolationDetector;
use crate::detect::datarace::DataRaceDetector;
use crate::detect::deadlock::DeadlockDetector;
#[cfg(feature = "atomic-violation")]
use crate::net::{core::Net, structure::TransitionType};
use crate::options::{DetectorKind, Options, StopAfter};
#[cfg(feature = "atomic-violation")]
use crate::report::{AtomicOperation, ViolationPattern};
use crate::report::{AtomicReport, DeadlockReport, RaceReport};
use crate::translate::callgraph::CallGraph;
use crate::translate::petri_net::PetriNet;
use crate::util::mem_watcher::MemoryWatcher;
use log::{debug, error, info};
use rayon::join;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{Instance, TyCtxt};
use std::fmt::{Debug, Formatter, Result};
use std::path::PathBuf;
#[cfg(feature = "atomic-violation")]
use std::time::Instant;

#[derive(Clone)]
pub struct PTACallbacks {
    pub options: Options,
    pub output_directory: PathBuf,
    test_run: bool,
}

impl PTACallbacks {
    pub fn new(options: Options) -> Self {
        let diagnostics_output = if let Some(output) = options.output.clone() {
            let mut path = PathBuf::from(output);
            path.push(&options.crate_name);
            path
        } else {
            let mut path = PathBuf::from("/tmp");
            path.push(&options.crate_name);
            path
        };

        std::fs::create_dir_all(&diagnostics_output).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to create output directory: {}", e);
        });

        Self {
            options,
            output_directory: diagnostics_output,
            test_run: false,
        }
    }
}

impl Debug for PTACallbacks {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        "PTACallbacks".fmt(f)
    }
}

impl Default for PTACallbacks {
    fn default() -> Self {
        Self::new(Options::default())
    }
}

impl rustc_driver::Callbacks for PTACallbacks {
    fn config(&mut self, config: &mut rustc_interface::interface::Config) {
        config.opts.optimize = rustc_session::config::OptLevel::No;
        config.opts.debuginfo = rustc_session::config::DebugInfo::None;

        let file_name = config
            .input
            .source_name()
            .prefer_remapped_unconditionally()
            .to_string();

        debug!("Processing input file: {}", file_name);
        if config.opts.test {
            debug!("in test only mode");
        }
    }

    fn after_analysis<'tcx>(
        &mut self,
        compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        compiler.sess.dcx().abort_if_errors();
        if self
            .output_directory
            .to_str()
            .expect("valid string")
            .contains("/build/")
        {
            return Compilation::Continue;
        }

        // 检查是否在 MIR 后停止(在分析之前)
        if self.options.stop_after == StopAfter::AfterMir {
            log::info!("停止分析:在 MIR 输出后停止");
            return Compilation::Stop;
        }

        self.analyze_with_pta(compiler, tcx);

        // 如果设置了停止点,在分析后停止编译
        if self.options.stop_after != StopAfter::None || self.test_run {
            Compilation::Stop
        } else {
            Compilation::Continue
        }
    }
}

impl PTACallbacks {
    fn analyze_with_pta<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'tcx>) {
        let mut mem_watcher = MemoryWatcher::new();
        mem_watcher.start();

        if tcx.sess.opts.unstable_opts.no_codegen || !tcx.sess.opts.output_types.should_codegen() {
            return;
        }

        let cgus = tcx.collect_and_partition_mono_items(()).codegen_units;
        let instances: Vec<Instance<'tcx>> = cgus
            .iter()
            .flat_map(|cgu| {
                cgu.items().iter().filter_map(|(mono_item, _)| {
                    if let MonoItem::Fn(instance) = mono_item {
                        Some(*instance)
                    } else {
                        None
                    }
                })
            })
            .collect();

        let mut callgraph = CallGraph::new();
        let key_api_regex = crate::translate::structure::KeyApiRegex::new(&self.options.config);
        callgraph.analyze(instances.clone(), tcx, &key_api_regex);

        // 输出 MIR dot(如果启用)
        if self.options.dump_options.dump_mir {
            self.dump_mir_dots(tcx, &instances);
        }

        // 检查是否在调用图后停止
        if self.options.stop_after == StopAfter::AfterCallGraph {
            log::info!("停止分析:在调用图构建后停止");
            return;
        }

        let mut pn = PetriNet::new(self.options.clone(), tcx, &callgraph);
        pn.construct();

        if self.options.config.reduce_net {
            use crate::net::reduce::{reduce_in_place, ReductionOptions};
            match reduce_in_place(&mut pn.net, ReductionOptions::default()) {
                Ok(result) => {
                    log::info!(
                        "Petri net reduced: {} steps (loops/sequences/intermediate)",
                        result.steps.len()
                    );
                }
                Err(e) => {
                    log::warn!("Petri net reduction failed: {}, continuing without reduction", e);
                }
            }
        }

        // 在构建状态图之前执行连通性诊断
        pn.net.log_diagnostics();

        // 如果启用了诊断输出,保存诊断报告到文件
        if self.options.dump_options.dump_petri_net {
            let report = pn.net.diagnose_connectivity();
            if report.has_issues() {
                let report_path = self.output_directory.join("petri_net_diagnostics.txt");
                if let Err(err) = report.save_to_file(report_path.to_str().unwrap_or("")) {
                    error!("failed to save diagnostic report: {err}");
                }
            }
        }

        // 检查是否在指针分析后停止
        if self.options.stop_after == StopAfter::AfterPointsTo
            || matches!(self.options.detector_kind, DetectorKind::PointsTo)
        {
            log::info!("停止分析:在指针分析后停止");
            let sg_config = StateGraphConfig {
                state_limit: self.options.config.state_limit,
                include_zero_tokens: false,
                use_por: self.options.config.por_enabled,
            };
            self.handle_visualizations(
                &callgraph,
                &pn,
                &StateGraph::with_config(&pn.net, sg_config),
                &instances,
            );
            return;
        }

        let sg_config = StateGraphConfig {
            state_limit: self.options.config.state_limit,
            include_zero_tokens: false,
            use_por: self.options.config.por_enabled,
        };
        let state_graph = StateGraph::with_config(&pn.net, sg_config);
        if state_graph.truncated {
            log::warn!(
                "状态空间已截断 (limit={:?}), 分析结果可能不完整",
                self.options.config.state_limit
            );
        }

        // 检查是否在状态图后停止
        if self.options.stop_after == StopAfter::AfterStateGraph {
            log::info!("停止分析:在状态图构建后停止");
            self.handle_visualizations(&callgraph, &pn, &state_graph, &instances);
            return;
        }

        self.handle_visualizations(&callgraph, &pn, &state_graph, &instances);
        #[cfg(feature = "atomic-violation")]
        self.run_detectors(&pn, &state_graph);
        #[cfg(not(feature = "atomic-violation"))]
        self.run_detectors(&state_graph);

        mem_watcher.stop();
    }

    fn handle_visualizations<'analysis, 'tcx>(
        &self,
        callgraph: &CallGraph<'tcx>,
        pn: &PetriNet<'analysis, 'tcx>,
        state_graph: &StateGraph,
        instances: &[Instance<'tcx>],
    ) {
        let dump = &self.options.dump_options;

        if dump.dump_call_graph {
            if let Err(err) = callgraph.write_dot(self.output_directory.join("callgraph.dot")) {
                error!("failed to write call graph dot file: {err}");
            } else {
                info!("call graph dot exported");
            }
        }

        if dump.dump_state_graph {
            if let Err(err) = state_graph.write_dot(self.output_directory.join("stategraph.dot")) {
                error!("failed to write state graph dot file: {err}");
            } else {
                info!("state graph dot exported");
            }
        }

        if dump.dump_petri_net {
            if let Err(err) = pn.net.write_dot(self.output_directory.join("petrinet.dot")) {
                error!("failed to write Petri net dot file: {err}");
            } else {
                info!("petri net dot exported");
            }
        }
        if dump.dump_unsafe_info {
            todo!()
        }
        if dump.dump_points_to || matches!(self.options.detector_kind, DetectorKind::PointsTo) {
            pn.alias.borrow_mut().ensure_pts_for_instances(instances);
            let report = pn.alias.borrow().format_points_to_report();
            let path = self.output_directory.join("points_to_report.txt");
            if let Err(err) = std::fs::write(&path, report) {
                error!("failed to write points-to report to {:?}: {err}", path);
            } else {
                info!("points-to report exported to {:?}", path);
            }
        }
    }

    fn dump_mir_dots<'tcx>(&self, tcx: TyCtxt<'tcx>, instances: &[Instance<'tcx>]) {
        use crate::util::mir_dot::write_mir_dot;

        let mir_dir = self.output_directory.join("mir");
        std::fs::create_dir_all(&mir_dir).unwrap_or_else(|e| {
            error!("Failed to create MIR output directory: {}", e);
        });

        for instance in instances {
            let def_id = instance.def_id();
            if !tcx.is_mir_available(def_id) {
                continue;
            }

            let body = tcx.optimized_mir(def_id);
            if body.source.promoted.is_some() {
                continue;
            }

            let fn_name = crate::util::format_name(def_id);
            let safe_fn_name = fn_name
                .replace(':', "_")
                .replace('-', "_")
                .replace('.', "_")
                .replace('/', "_");
            let mir_path = mir_dir.join(format!("{}.dot", safe_fn_name));

            if let Err(err) = write_mir_dot(tcx, def_id, body, &mir_path) {
                error!("Failed to write MIR dot for {}: {}", fn_name, err);
            } else {
                info!("MIR dot exported: {}", mir_path.display());
            }
        }
    }

    #[cfg(not(feature = "atomic-violation"))]
    fn run_detectors(&self, state_graph: &StateGraph) {
        match self.options.detector_kind {
            DetectorKind::Deadlock => {
                self.run_deadlock_detector(state_graph);
            }
            DetectorKind::DataRace => {
                self.run_datarace_detector(state_graph);
            }
            DetectorKind::PointsTo => {
                // Points-to mode returns early; this arm is unreachable
            }
            DetectorKind::AtomicityViolation => {
                #[cfg(feature = "atomic-violation")]
                {
                    self.run_atomic_detector(state_graph);
                }
                #[cfg(not(feature = "atomic-violation"))]
                {
                    log::warn!(
                        "请求执行原子性违背检测,但未启用 atomic-violation feature,分析被跳过."
                    );
                }
            }
            DetectorKind::All => {
                #[cfg(feature = "atomic-violation")]
                {
                    log::info!(
                        "由于数据竞争与原子性违背检测互斥,--mode all 默认执行数据竞争分析；如需原子性分析请使用 --mode atomic 并启用 feature."
                    );
                }
                join(
                    || self.run_deadlock_detector(state_graph),
                    || self.run_datarace_detector(state_graph),
                );
            }
        }
    }

    #[cfg(feature = "atomic-violation")]
    fn run_detectors(&self, pn: &PetriNet, state_graph: &StateGraph) {
        match self.options.detector_kind {
            DetectorKind::Deadlock => {
                self.run_deadlock_detector(state_graph);
            }
            DetectorKind::DataRace => {
                self.run_datarace_detector(state_graph);
            }
            DetectorKind::PointsTo => {
                // Points-to mode returns early; this arm is unreachable
            }
            DetectorKind::AtomicityViolation => {
                self.run_atomic_detector(pn);
            }
            DetectorKind::All => {
                info!(
                    "由于数据竞争与原子性违背检测互斥,--mode all 默认执行数据竞争分析；如需原子性分析请使用 --mode atomic 并启用 feature."
                );
                join(
                    || self.run_deadlock_detector(state_graph),
                    || self.run_datarace_detector(state_graph),
                );
            }
        }
    }

    fn run_deadlock_detector(&self, state_graph: &StateGraph) {
        let report = DeadlockDetector::new(state_graph).detect();
        self.log_deadlock(&report);
        self.write_report(self.output_directory.join("deadlock_report.txt"), |path| {
            report.save_to_file(path)
        });
    }

    fn run_datarace_detector(&self, state_graph: &StateGraph) {
        let report = DataRaceDetector::new(state_graph).detect();
        self.log_datarace(&report);
        self.write_report(self.output_directory.join("datarace_report.txt"), |path| {
            report.save_to_file(path)
        });
    }

    #[cfg(not(feature = "atomic-violation"))]
    #[allow(dead_code)]
    fn run_atomic_detector(&self, state_graph: &StateGraph) {
        let report = AtomicityViolationDetector::new(state_graph).detect();
        self.log_atomic(&report);
        self.write_report(self.output_directory.join("atomicity_report.txt"), |path| {
            report.save_to_file(path)
        });
    }

    #[cfg(feature = "atomic-violation")]
    fn run_atomic_detector(&self, pn: &PetriNet) {
        let net = &pn.net;
        let init = marking_from_places(net);
        let start = Instant::now();
        let witnesses = detect_atomicity_violations(net, &init, 200_000, 10_000);
        let elapsed = start.elapsed();

        print_witnesses(net, &witnesses);

        let mut report = AtomicReport::new("Petri Net Atomic Violation Detector".to_string());
        report.analysis_time = elapsed;
        report.has_violation = !witnesses.is_empty();
        report.violation_count = witnesses.len();
        report.violations = witnesses
            .iter()
            .map(|w| witness_to_pattern(net, w))
            .collect();

        self.log_atomic(&report);
        self.write_report(self.output_directory.join("atomicity_report.txt"), |path| {
            report.save_to_file(path)
        });
    }

    fn write_report<F>(&self, path: PathBuf, write: F)
    where
        F: FnOnce(&str) -> std::io::Result<()>,
    {
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                error!("failed to create report directory {:?}: {err}", parent);
                return;
            }
        }

        match path.to_str() {
            Some(path_str) => {
                if let Err(err) = write(path_str) {
                    error!("failed to persist report {:?}: {err}", path);
                }
            }
            None => error!("report path contains invalid UTF-8: {:?}", path),
        }
    }

    fn log_deadlock(&self, report: &DeadlockReport) {
        if report.has_deadlock {
            info!(
                "deadlock analysis detected {} deadlocks",
                report.deadlock_count
            );
        } else {
            info!("deadlock analysis completed: no deadlocks detected");
        }
    }

    fn log_datarace(&self, report: &RaceReport) {
        if report.has_race {
            info!(
                "data race analysis detected {} potential races",
                report.race_count
            );
        } else {
            info!("data race analysis completed: no races detected");
        }
    }

    #[allow(dead_code)]
    fn log_atomic(&self, report: &AtomicReport) {
        if report.has_violation {
            info!(
                "atomicity analysis detected {} violation patterns",
                report.violation_count
            );
        } else {
            info!("atomicity analysis completed: no violations detected");
        }
    }
}

#[cfg(feature = "atomic-violation")]
fn witness_to_pattern(net: &Net, witness: &Witness) -> ViolationPattern {
    let (alias, trace_slice) = match witness {
        Witness::AV1 {
            alias, trace_slice, ..
        }
        | Witness::AV2 {
            alias, trace_slice, ..
        }
        | Witness::AV3 {
            alias, trace_slice, ..
        } => (*alias, trace_slice),
    };

    let mut load_op: Option<AtomicOperation> = None;
    let mut store_ops: Vec<AtomicOperation> = Vec::new();

    for transition_id in trace_slice {
        let transition = &net.transitions[*transition_id];
        match &transition.transition_type {
            TransitionType::AtomicLoad(alias_id, ordering, span, tid) => {
                if load_op.is_none() {
                    load_op = Some(AtomicOperation {
                        operation_type: format!("load@tid{tid}"),
                        ordering: format!("{ordering:?}"),
                        variable: format!("{alias_id:?}"),
                        location: span.clone(),
                    });
                }
            }
            TransitionType::AtomicStore(alias_id, ordering, span, tid) => {
                store_ops.push(AtomicOperation {
                    operation_type: format!("store@tid{tid}"),
                    ordering: format!("{ordering:?}"),
                    variable: format!("{alias_id:?}"),
                    location: span.clone(),
                });
            }
            TransitionType::AtomicCmpXchg(alias_id, success, _failure, span, tid) => {
                store_ops.push(AtomicOperation {
                    operation_type: format!("cas_store@tid{tid}"),
                    ordering: format!("{success:?}"),
                    variable: format!("{alias_id:?}"),
                    location: span.clone(),
                });
            }
            _ => {}
        }
    }

    let load_op = load_op.unwrap_or_else(|| AtomicOperation {
        operation_type: "load".to_string(),
        ordering: "N/A".to_string(),
        variable: format!("{:?}", alias),
        location: String::from("<unknown>"),
    });

    ViolationPattern { load_op, store_ops }
}
