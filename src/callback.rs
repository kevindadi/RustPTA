//! The main functionality: callbacks for rustc plugin systems.
//! Inspired by <https://github.com/facebookexperimental/MIRAI/blob/9cf3067309d591894e2d0cd9b1ee6e18d0fdd26c/checker/src/callbacks.rs>
extern crate rustc_driver;
extern crate rustc_hir;

use crate::detect::atomicity_violation::AtomicityViolationDetector;
use crate::detect::datarace::DataRaceDetector;
use crate::detect::deadlock::DeadlockDetector;
use crate::extern_tools::lola::LolaAnalyzer;
use crate::extern_tools::tina::TinaAnalyzer;
use crate::graph::callgraph::CallGraph;
use crate::graph::mir_flml::{AnalysisConfig, MirToFLMLConverter};
use crate::graph::pn::PetriNet;
use crate::graph::state_graph::StateGraph;
use crate::options::{AnalysisTool, Options, OwnCrateType};
use crate::util::mem_watcher::MemoryWatcher;
use crate::util::{parse_api_spec, ApiSpec};
use crate::DetectorKind;
use log::debug;
use rustc_driver::Compilation;
use rustc_hir::def_id::DefId;
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
        // Construct default diagnostic output path
        let diagnostics_output = if let Some(output) = options.output.clone() {
            let mut path = PathBuf::from(output);
            path.push(&options.crate_name);
            path
        } else {
            let mut path = PathBuf::from("/tmp");
            path.push(&options.crate_name);
            path
        };

        // Ensure directory exists
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
            //.prefer_remapped() // nightly-2023-09-13
            .prefer_remapped_unconditionaly()
            .to_string();

        debug!("Processing input file: {}", file_name);
        if config.opts.test {
            debug!("in test only mode");
            // self.options.test_only = true;
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
            // No need to analyze a build script, but do generate code.
            return Compilation::Continue;
        }

        self.analyze_with_pta(compiler, tcx);

        if self.test_run {
            // We avoid code gen for test cases because LLVM is not used in a thread safe manner.
            Compilation::Stop
        } else {
            Compilation::Continue
        }
    }
}

impl PTACallbacks {
    fn analyze_with_pta<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'tcx>) {
        let mut mem_watcher = MemoryWatcher::default();
        let _ = mem_watcher.start();

        if tcx.sess.opts.unstable_opts.no_codegen || !tcx.sess.opts.output_types.should_codegen() {
            return;
        }

        let MonoItemPartitions {
            codegen_units,
            all_mono_items,
            ..
        } = tcx.collect_and_partition_mono_items(());
        let instances: Vec<Instance<'tcx>> = codegen_units
            .iter()
            .filter_map(|cgu| {
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

        if self.options.crate_type == OwnCrateType::Lib {
            let api_spec = parse_api_spec(self.options.lib_apis_path.as_ref().unwrap())
                .unwrap_or_else(|e| {
                    log::error!("Failed to parse api spec: {}", e);
                    ApiSpec::default()
                });

            let mut pn = PetriNet::new(
                &self.options,
                tcx,
                &callgraph,
                api_spec,
                false,
                self.output_directory.clone(),
                true,
                false,
                false,
            );
            pn.construct();
            pn.save_petri_net_to_file();
            let terminal_states = pn.get_terminal_states();
            // log::info!("apis_marks: {:?}", pn.api_marks);
            let mut state_graph = StateGraph::new(
                pn.net.clone(),
                pn.get_current_mark(),
                pn.function_counter.clone(),
                self.options.clone(),
                terminal_states,
            );
            for (api_name, initial_mark) in pn.api_marks.iter() {
                // TODO: API reachability graph refactoring
            }

            mem_watcher.stop();
            // log::info!("deadlock state: {}", state_graph.detect_api_deadlock());
            return;
        }

        //  TODO: mode as a parameter for the network
        match &self.options.detector_kind {
            DetectorKind::FLML => {
                // Generate FLML intermediate representation
                log::info!("Generating FLML intermediate representation");

                // Choose configuration based on options
                let config = AnalysisConfig::deadlock_detection(); // Use deadlock detection configuration by default
                let mut converter = MirToFLMLConverter::new(tcx, config);

                // Convert all functions
                for instance in instances.iter() {
                    if tcx.is_mir_available(instance.def_id()) {
                        let body = tcx.optimized_mir(instance.def_id());
                        converter.convert_function(instance.def_id(), body);
                        log::debug!(
                            "Converted function: {}",
                            tcx.def_path_str(instance.def_id())
                        );
                    }
                }

                // Export FLML IR
                if self.options.dump_options.dump_flml {
                    match converter.export_to_json() {
                        Ok(json_output) => {
                            let output_path = self.output_directory.join("flml_ir.json");
                            std::fs::write(&output_path, json_output).unwrap_or_else(|e| {
                                log::error!("Failed to write FLML IR to file: {}", e);
                            });
                            log::info!("FLML IR exported to: {}", output_path.display());
                        }
                        Err(e) => {
                            log::error!("Failed to export FLML IR to JSON: {}", e);
                        }
                    }
                }

                let flml_ir = converter.get_flml_ir();
                log::info!(
                    "FLML IR generated with {} nodes and {} edges",
                    flml_ir.graph.node_count(),
                    flml_ir.graph.edge_count()
                );
            }
            DetectorKind::DataRace => {
                let mut pn = PetriNet::new(
                    &self.options,
                    tcx,
                    &callgraph,
                    ApiSpec::default(),
                    true,
                    self.output_directory.clone(),
                    false,
                    false,
                    true,
                );

                pn.construct();
                pn.save_petri_net_to_file();
                let terminal_states = pn.get_terminal_states();
                let mut state_graph = StateGraph::new(
                    pn.net.clone(),
                    pn.get_current_mark(),
                    pn.function_counter.clone(),
                    self.options.clone(),
                    terminal_states,
                );
                state_graph.generate_states();
                let detector = DataRaceDetector::new(&state_graph);
                let data_races = detector.detect();
                //log::info!("Data Race: {}", data_races);
                println!("Data Race: {}", data_races);
                if self.options.dump_options.dump_points_to {
                    pn.alias.borrow_mut().print_all_points_to_relations();
                }
            }
            DetectorKind::AtomicityViolation => {
                // Collect atomic variable and operation information
                log::debug!("Starting atomic operation collection");
                let mut pn = PetriNet::new(
                    &self.options,
                    tcx,
                    &callgraph,
                    ApiSpec::default(),
                    true,
                    self.output_directory.clone(),
                    false,
                    true,
                    false,
                );

                pn.construct();
                pn.save_petri_net_to_file();
                let terminal_states = pn.get_terminal_states();
                let mut state_graph = StateGraph::new(
                    pn.net.clone(),
                    pn.get_current_mark(),
                    pn.function_counter.clone(),
                    self.options.clone(),
                    terminal_states,
                );
                state_graph.generate_states();
                let detector = AtomicityViolationDetector::new(&state_graph);
                let atomicity_violation = detector.detect();
                //log::info!("atomicity_violation: {}", atomicity_violation);
                println!("atomicity_violation: {}", atomicity_violation);

                // log::info!("atomic_races: {}", detector.generate_atomic_races());

                if self.options.dump_options.dump_points_to {
                    pn.alias.borrow_mut().print_all_points_to_relations();
                }
            }
            _ => {
                let mut pn = PetriNet::new(
                    &self.options,
                    tcx,
                    &callgraph,
                    ApiSpec::default(),
                    false,
                    self.output_directory.clone(),
                    true,
                    false,
                    false,
                );

                pn.construct();
                pn.save_petri_net_to_file();

                match self.options.analysis_tool {
                    AnalysisTool::LoLA => {
                        let analyzer = LolaAnalyzer::new(
                            "lola".to_string(),
                            "pn.lola".to_string(),
                            self.output_directory.clone(),
                        );
                        log::info!("Lola Result: {:?}", analyzer.analyze_petri_net(&pn));
                    }
                    AnalysisTool::Tina => {
                        let analyzer = TinaAnalyzer::new(
                            "tina".to_string(),
                            "pn.tina".to_string(),
                            self.output_directory.clone(),
                        );
                        println!("Tina Result: {}", analyzer.get_analysis_info().unwrap());
                    }
                    _ => {
                        let mut state_graph = StateGraph::new(
                            pn.net.clone(),
                            pn.get_current_mark(),
                            pn.function_counter.clone(),
                            self.options.clone(),
                            pn.get_terminal_states(),
                        );

                        state_graph.generate_states();
                        state_graph.dot().unwrap();
                        let deadlock_detector = DeadlockDetector::new(&state_graph);
                        let result = deadlock_detector.detect();
                        //log::info!("deadlock state: {}", result);
                        println!("deadlock state: {}", result);
                    }
                }

                if self.options.dump_options.dump_points_to {
                    pn.alias.borrow_mut().print_all_points_to_relations();
                }
            }
        }
        mem_watcher.stop();
    }
}
