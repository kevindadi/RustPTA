use crate::net::core::Net;
use crate::net::structure::{Place, PlaceType};
use crate::net::PlaceId;
use crate::translate::callgraph::CallGraph;
use crate::translate::mir_to_pn::BodyToPetriNet;
use crate::util::format_name;
use crate::Options;

use petgraph::visit::IntoNodeReferences;
use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::memory::pointsto::AliasAnalysis;

pub struct PetriNet<'compilation, 'pn, 'tcx> {
    options: &'compilation Options,
    _output_directory: PathBuf,
    tcx: TyCtxt<'tcx>,
    pub net: Net,
    callgraph: &'pn CallGraph<'tcx>,
    pub alias: RefCell<AliasAnalysis<'pn, 'tcx>>,
    pub function_counter: HashMap<DefId, (PlaceId, PlaceId)>,
    pub entry_exit: Option<(PlaceId, PlaceId)>,
}

impl<'compilation, 'pn, 'tcx> PetriNet<'compilation, 'pn, 'tcx> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        options: &'compilation Options,
        tcx: TyCtxt<'tcx>,
        callgraph: &'pn CallGraph<'tcx>,
        _api_spec: crate::util::ApiSpec,
        av: bool,
        output_directory: PathBuf,
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, callgraph, av));
        Self {
            options,
            _output_directory: output_directory,
            tcx,
            net: Net::empty(),
            callgraph,
            alias,
            function_counter: HashMap::new(),
            entry_exit: None,
        }
    }

    pub fn construct(&mut self) {
        self.construct_function_places();

        for (_node_idx, caller) in self.callgraph.graph.node_references() {
            let def_id = caller.instance().def_id();
            if !self.tcx.is_mir_available(def_id) {
                continue;
            }
            if !format_name(def_id).starts_with(&self.options.crate_name) {
                continue;
            }
            let Some(entry_exit) = self.function_counter.get(&def_id).copied() else {
                continue;
            };
            let body = self.tcx.optimized_mir(def_id);
            if body.source.promoted.is_some() {
                continue;
            }
            let mut translator =
                BodyToPetriNet::new(caller.instance(), body, self.tcx, &mut self.net, entry_exit);
            translator.translate();
        }
    }

    fn construct_function_places(&mut self) {
        for node_idx in self.callgraph.graph.node_indices() {
            let func_node = self.callgraph.graph.node_weight(node_idx).unwrap();
            let instance = func_node.instance();
            let def_id = instance.def_id();
            let func_name = format_name(def_id);
            if !func_name.starts_with(&self.options.crate_name) {
                continue;
            }
            if self.function_counter.contains_key(&def_id) {
                continue;
            }

            let with_initial_token =
                matches!(self.options.crate_type, crate::options::OwnCrateType::Bin)
                    && matches!(self.tcx.entry_fn(()), Some((entry, _)) if entry == def_id);

            let start_place = self.create_place(
                format!("{}_start", func_name),
                if with_initial_token { 1 } else { 0 },
                PlaceType::FunctionStart,
            );
            let end_place =
                self.create_place(format!("{}_end", func_name), 0, PlaceType::FunctionEnd);

            self.function_counter
                .insert(def_id, (start_place, end_place));

            if with_initial_token {
                self.entry_exit = Some((start_place, end_place));
            }
        }

        if self.entry_exit.is_none() {
            // provide a default entry/exit for library crates
            let start_place =
                self.create_place("crate_start".to_string(), 0, PlaceType::FunctionStart);
            let end_place = self.create_place("crate_end".to_string(), 0, PlaceType::FunctionEnd);
            self.entry_exit = Some((start_place, end_place));
        }
    }

    fn create_place(&mut self, name: String, tokens: u64, place_type: PlaceType) -> PlaceId {
        let place = Place::new(name, tokens, u64::MAX, place_type, String::new());
        self.net.add_place(place)
    }
}
