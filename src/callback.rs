//! The main functionality: callbacks for rustc plugin systems.
//! Inspired by <https://github.com/facebookexperimental/MIRAI/blob/9cf3067309d591894e2d0cd9b1ee6e18d0fdd26c/checker/src/callbacks.rs>
extern crate rustc_driver;
extern crate rustc_hir;

use std::io::Write;
use std::path::PathBuf;

use crate::graph::callgraph::CallGraph;
use crate::graph::petri_net::PetriNet;
use crate::options::Options;
use log::debug;
use rustc_driver::Compilation;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_interface::interface;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{Instance, ParamEnv, TyCtxt};
use std::fmt::{Debug, Formatter, Result};

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
            .prefer_remapped() // nightly-2023-09-13
            //.prefer_remapped_unconditionaly()
            .to_string();

        debug!("Processing input file: {}", self.file_name);
        if config.opts.test {
            debug!("in test only mode");
            // self.options.test_only = true;
        }
        config.crate_cfg.insert(("pta".to_string(), None));
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
        queries: &'tcx rustc_interface::Queries<'tcx>,
    ) -> rustc_driver::Compilation {
        compiler.session().abort_if_errors();
        if self
            .output_directory
            .to_str()
            .expect("valid string")
            .contains("/build/")
        {
            // No need to analyze a build script, but do generate code.
            return Compilation::Continue;
        }
        queries.global_ctxt().unwrap().enter(|tcx| {
            self.analyze_with_pta(compiler, tcx);
        });
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
        // Skip crates by names (white or black list).
        // let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
        let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();

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
        let mut callgraph = CallGraph::new();
        let param_env = ParamEnv::reveal_all();
        callgraph.analyze(instances.clone(), tcx, param_env);

        let callgraph_output = callgraph.dot();
        let callgraph_path = "callgraph.dot";
        let mut callgraph_file = std::fs::File::create(callgraph_path).unwrap();
        callgraph_file
            .write_all(callgraph_output.as_bytes())
            .expect("Unable to write callgraph!");

        log::debug!("analysi crate is {:?}", self.options.crate_name);
        if !crate_name.eq(&self.options.crate_name) {
            log::debug!("No conversion is required for this crate {:?}!", crate_name);
            return;
        }

        log::debug!("convert {} to Petri Net!", crate_name);
        let mut pn = PetriNet::new(&self.options, tcx, param_env, &callgraph);
        pn.construct();

        pn.save_petri_net_to_file();
        let _ = pn.generate_state_graph();
        let result = pn.check_deadlock();
        println!("deadlock state: {}", result);
    }
}
