//! The main functionality: callbacks for rustc plugin systems.
//! Inspired by <https://github.com/facebookexperimental/MIRAI/blob/9cf3067309d591894e2d0cd9b1ee6e18d0fdd26c/checker/src/callbacks.rs>
extern crate rustc_driver;
extern crate rustc_hir;

use crate::concurrency::atomic::AtomicCollector;
use crate::graph::callgraph::CallGraph;
use crate::graph::cpn::ColorPetriNet;
use crate::graph::cpn_state_graph::CpnStateGraph;
use crate::graph::pn::PetriNet;
use crate::graph::state_graph::StateGraph;
use crate::memory::unsafe_memory::UnsafeAnalyzer;
use crate::options::{Options, OwnCrateType};
use crate::utils::{format_name, parse_api_spec, ApiSpec};
use crate::DetectorKind;
use log::debug;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{Instance, TyCtxt};
use serde_json::{self, json};
use std::fmt::{Debug, Formatter, Result};
use std::path::PathBuf;

#[derive(Clone)]
pub struct PTACallbacks {
    pub options: Options,
    file_name: String,
    output_directory: PathBuf,
    test_run: bool,
}

impl PTACallbacks {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            file_name: String::new(),
            output_directory: PathBuf::default(),
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
        self.file_name = config
            .input
            .source_name()
            //.prefer_remapped() // nightly-2023-09-13
            .prefer_remapped_unconditionaly()
            .to_string();

        debug!("Processing input file: {}", self.file_name);
        if config.opts.test {
            debug!("in test only mode");
            // self.options.test_only = true;
        }
        //config.crate_cfg.insert("pta".to_string(), None);
        match &config.output_dir {
            None => {
                self.output_directory = std::env::temp_dir();
                self.output_directory.pop();
            }
            Some(path_buf) => self.output_directory.push(path_buf.as_path()),
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
        // queries.global_ctxt().unwrap().peek_mut().enter(|tcx| {
        //     self.analyze_with_lockbud(compiler, tcx);
        // });

        self.analyze_with_pta(compiler, tcx);

        if self.test_run {
            // We avoid code gen for test cases because LLVM is not used in a thread safe manner.
            Compilation::Stop
        } else {
            // Although LockBud is only a checker, cargo still needs code generation to work.
            Compilation::Continue
        }
    }
}

impl PTACallbacks {
    fn analyze_with_pta<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: TyCtxt<'tcx>) {
        if tcx.sess.opts.unstable_opts.no_codegen || !tcx.sess.opts.output_types.should_codegen() {
            return;
        }

        let cgus = tcx.collect_and_partition_mono_items(()).1;
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
        // log::info!("Crate name: {:?}", self.options.crate_name);
        // log::info!("---------------------------------------");
        // for instance in instances.iter() {
        //     log::info!("instance: {:?}", format_name(instance.def_id()));
        // }
        let mut callgraph = CallGraph::new();
        callgraph.analyze(instances.clone(), tcx);

        if self.options.crate_type == OwnCrateType::Lib {
            let api_spec = parse_api_spec(self.options.lib_apis_path.as_ref().unwrap())
                .unwrap_or_else(|e| {
                    log::error!("Failed to parse api spec: {}", e);
                    ApiSpec::default()
                });

            let mut pn = PetriNet::new(&self.options, tcx, &callgraph, api_spec, false);
            pn.construct();
            pn.save_petri_net_to_file();
            // log::info!("apis_marks: {:?}", pn.api_marks);
            let mut state_graph = StateGraph::new(pn.net.clone(), pn.get_current_mark());
            for (api_name, initial_mark) in pn.api_marks.iter() {
                state_graph.generate_states_with_api(api_name.clone(), initial_mark.clone());
            }

            log::info!("deadlock state: {}", state_graph.detect_api_deadlock());
            return;
        }
        log::info!("{}", callgraph.format_spawn_calls());
        match &self.options.detector_kind {
            DetectorKind::DataRace => {
                let unsafe_analyzer =
                    UnsafeAnalyzer::new(tcx, &callgraph, self.options.crate_name.clone());
                let (unsafe_info, unsafe_data) = unsafe_analyzer.analyze();
                unsafe_info.iter().for_each(|(def_id, info)| {
                    log::info!(
                        "{}:\n{}",
                        format_name(*def_id),
                        serde_json::to_string_pretty(&json!({
                            "unsafe_fn": info.is_unsafe_fn,
                            "unsafe_blocks": info.unsafe_blocks,
                            "unsafe_places": info.unsafe_places
                        }))
                        .unwrap()
                    )
                });
                log::info!("unsafe_data size: {:?}", unsafe_data.unsafe_places.len());
                let mut cpn =
                    ColorPetriNet::new(self.options.clone(), tcx, &callgraph, unsafe_data, false);
                cpn.construct();
                cpn.cpn_to_dot("cpn.dot").unwrap();

                let mut state_graph = CpnStateGraph::new(cpn.net.clone(), cpn.get_marking());
                state_graph.generate_states();
                state_graph
                    .race_info
                    .lock()
                    .unwrap()
                    .iter()
                    .for_each(|race_info| {
                        log::info!(
                            "Race {:?}:\n{}",
                            serde_json::to_string(&json!({
                                "unsafe_transitions": race_info.transitions,
                            })),
                            serde_json::to_string_pretty(&json!({
                                "operations": race_info.span_str,
                            }))
                            .unwrap()
                        )
                    });
            }
            DetectorKind::AtomicityViolation => {
                // 收集atomic变量和操作信息
                log::debug!("Starting atomic operation collection");
                let mut atomic_collector =
                    AtomicCollector::new(tcx, &callgraph, self.options.crate_name.clone());
                let atomic_vars = atomic_collector.analyze();

                // 输出收集到的atomic信息
                atomic_collector.to_json_pretty().unwrap();
                let mut pn =
                    PetriNet::new(&self.options, tcx, &callgraph, ApiSpec::default(), true);

                if !atomic_vars.is_empty() {
                    pn.add_atomic_places(&atomic_vars);
                }

                pn.construct();
                pn.save_petri_net_to_file();

                let mut state_graph = StateGraph::new(pn.net.clone(), pn.get_current_mark());
                state_graph.generate_states();

                state_graph.detect_atomic_violation();
                if self.options.dump_options.dump_points_to {
                    pn.alias.borrow_mut().print_all_points_to_relations();
                }
            }
            _ => {
                let mut pn =
                    PetriNet::new(&self.options, tcx, &callgraph, ApiSpec::default(), false);
                pn.construct();
                pn.save_petri_net_to_file();

                let mut state_graph = StateGraph::new(pn.net.clone(), pn.get_current_mark());
                state_graph.generate_states();
                let result = state_graph.detect_deadlock_use_state_reachable_graph();
                log::info!("deadlock state: {}", result);
                state_graph.detect_deadlock_use_model_check();

                if self.options.dump_options.dump_points_to {
                    pn.alias.borrow_mut().print_all_points_to_relations();
                }
            }
        }
    }
}
