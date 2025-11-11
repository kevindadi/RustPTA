extern crate rustc_driver;
extern crate rustc_hir;

use crate::analysis::reachability::StateGraph;
use crate::detect::atomicity_violation::AtomicityViolationDetector;
use crate::detect::datarace::DataRaceDetector;
use crate::detect::deadlock::DeadlockDetector;
use crate::options::{DetectorKind, Options};
use crate::report::{AtomicReport, DeadlockReport, RaceReport};
use crate::translate::callgraph::CallGraph;
use crate::translate::petri_net::PetriNet;
use crate::util::mem_watcher::MemoryWatcher;
use log::{debug, error, info};
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{Instance, TyCtxt};
use std::fmt::{Debug, Formatter, Result};
use std::path::PathBuf;

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

        self.analyze_with_pta(compiler, tcx);

        if self.test_run {
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
        callgraph.analyze(instances.clone(), tcx);

        let mut pn = PetriNet::new(self.options.clone(), tcx, &callgraph);
        pn.construct();

        let state_graph = StateGraph::from_net(&pn.net);

        self.handle_visualizations(&callgraph, &pn, &state_graph);
        self.run_detectors(&state_graph);

        mem_watcher.stop();
    }

    fn handle_visualizations<'analysis, 'tcx>(
        &self,
        callgraph: &CallGraph<'tcx>,
        pn: &PetriNet<'analysis, 'tcx>,
        state_graph: &StateGraph,
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
        if dump.dump_points_to {
            todo!()
        }
    }

    fn run_detectors(&self, state_graph: &StateGraph) {
        let kind = &self.options.detector_kind;
        let out_dir = &self.output_directory;

        let run = |target: DetectorKind| -> bool {
            match kind {
                DetectorKind::All => true,
                DetectorKind::Deadlock => matches!(target, DetectorKind::Deadlock),
                DetectorKind::AtomicityViolation => {
                    matches!(target, DetectorKind::AtomicityViolation)
                }
                DetectorKind::DataRace => matches!(target, DetectorKind::DataRace),
            }
        };

        if run(DetectorKind::Deadlock) {
            let report = DeadlockDetector::new(state_graph).detect();
            self.log_deadlock(&report);
            self.write_report(out_dir.join("deadlock_report.txt"), |path| {
                report.save_to_file(path)
            });
        }

        if run(DetectorKind::DataRace) {
            let report = DataRaceDetector::new(state_graph).detect();
            self.log_datarace(&report);
            self.write_report(out_dir.join("datarace_report.txt"), |path| {
                report.save_to_file(path)
            });
        }

        if run(DetectorKind::AtomicityViolation) {
            let report = AtomicityViolationDetector::new(state_graph).detect();
            self.log_atomic(&report);
            self.write_report(out_dir.join("atomicity_report.txt"), |path| {
                report.save_to_file(path)
            });
        }
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
