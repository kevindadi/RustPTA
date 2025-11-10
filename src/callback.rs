extern crate rustc_driver;
extern crate rustc_hir;

// use crate::detect::atomicity_violation::AtomicityViolationDetector;
// use crate::detect::datarace::DataRaceDetector;
// use crate::detect::deadlock::DeadlockDetector;
use crate::options::{DumpOptions, Options};
use crate::translate::callgraph::CallGraph;
use crate::translate::petri_net::PetriNet;
use crate::util::mem_watcher::MemoryWatcher;
use log::debug;
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

        let mut pn = PetriNet::new(&self.options, tcx, &callgraph);

        pn.construct();

        // let mut state_graph = StateGraph::new(
        //     pn.net.clone(),
        //     pn.get_current_mark(),
        //     pn.function_counter.clone(),
        //     self.options.clone(),
        //     terminal_states,
        // );
        // state_graph.generate_states();


        match self.options.dump_options {
            DumpOptions { dump_call_graph: true, .. } => {
                callgraph.write_dot(&self.output_directory.join("callgraph.dot")).unwrap();
            }
            DumpOptions { dump_petri_net: true, .. } => {
                todo!()
            }
            DumpOptions { dump_unsafe_info: true, .. } => {
                todo!()
            }
            DumpOptions { dump_points_to: true, .. } => {
                todo!()
            }
            _ => {}
        }

        mem_watcher.stop();
    }
}
