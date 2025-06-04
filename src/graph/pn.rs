//! Petri Net construction and analysis module for Rust concurrent programs.
//!
//! This module provides the core functionality for converting Rust programs into
//! Petri Net models for deadlock and concurrency analysis. It handles the construction
//! of places, transitions, and their connections based on program control flow and
//! synchronization operations.
//!
//! Key components:
//! - PetriNet: Main structure that represents the Petri net model
//! - Place/Transition creation for various program constructs
//! - Lock dependency analysis using DFS traversal
//! - State reduction for optimization
//! - Integration with atomic operations, channels, and unsafe blocks
//!
//! The module supports analysis of:
//! - Mutex locks and unlocks
//! - Condition variables
//! - Atomic operations
//! - Channel operations
//! - Unsafe memory operations
//! - Function calls and control flow

use crate::concurrency::atomic::{AtomicCollector, AtomicOrdering};
use crate::concurrency::channel::{ChannelCollector, ChannelId, ChannelInfo, EndpointType};
use crate::graph::net_structure::{
    CallType, ControlType, DropType, KeyApiRegex, NetConfig, Transition,
};
use crate::memory::pointsto::AliasId;
use crate::memory::unsafe_memory::UnsafeAnalyzer;
use crate::options::OwnCrateType;
use crate::util::{format_name, ApiEntry, ApiSpec};
use crate::Options;
use anyhow::Result;
use log::debug;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction;
use petgraph::Graph;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use serde_json::json;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::mir_pn::BodyToPetriNet;
use super::net_structure::{PetriNetEdge, PetriNetError, PetriNetNode, Place, PlaceType};
use crate::concurrency::blocking::{
    BlockingCollector, CondVarId, LockGuardId, LockGuardMap, LockGuardTy,
};
use crate::memory::pointsto::{AliasAnalysis, ApproximateAliasKind};

/// Union-Find data structure for managing lock relationships
fn find(union_find: &HashMap<LockGuardId, LockGuardId>, x: &LockGuardId) -> LockGuardId {
    if let Some(parent) = union_find.get(x) {
        if parent != x {
            return find(union_find, parent);
        }
    }
    x.clone()
}

// Union-Find merge function
fn union(union_find: &mut HashMap<LockGuardId, LockGuardId>, x: &LockGuardId, y: &LockGuardId) {
    let root_x = find(union_find, x);
    let root_y = find(union_find, y);
    if root_x != root_y {
        union_find.insert(root_x, root_y);
    }
}

pub struct PetriNet<'compilation, 'pn, 'tcx> {
    options: &'compilation Options,
    output_directory: PathBuf,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    pub net: Graph<PetriNetNode, PetriNetEdge>,
    callgraph: &'pn CallGraph<'tcx>,
    pub alias: RefCell<AliasAnalysis<'pn, 'tcx>>,
    pub function_counter: HashMap<DefId, (NodeIndex, NodeIndex)>,
    pub function_vec: HashMap<DefId, Vec<NodeIndex>>,
    locks_counter: HashMap<LockGuardId, NodeIndex>,
    lock_info: LockGuardMap<'tcx>,
    // all condvars
    condvars: HashMap<CondVarId, NodeIndex>,
    pub api_spec: ApiSpec,
    pub api_marks: HashMap<String, HashSet<(NodeIndex, u8)>>,
    atomic_places: HashMap<AliasId, NodeIndex>,
    atomic_order_maps: HashMap<AliasId, AtomicOrdering>,
    pub entry_exit: (NodeIndex, NodeIndex),
    pub enable_block_collector: bool,
    pub enable_atomic_collector: bool,
    pub enbale_unsafe_collector: bool,
    pub unsafe_places: HashMap<AliasId, NodeIndex>,
    pub channel_places: HashMap<ChannelId, NodeIndex>,
}

impl<'compilation, 'pn, 'tcx> PetriNet<'compilation, 'pn, 'tcx> {
    pub fn new(
        options: &'compilation Options,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        callgraph: &'pn CallGraph<'tcx>,
        api_spec: ApiSpec,
        av: bool,
        output_directory: PathBuf,
        enable_block_collector: bool,
        enable_atomic_collector: bool,
        enbale_unsafe_collector: bool,
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, &callgraph, av));
        Self {
            options,
            tcx,
            output_directory,
            net: Graph::<PetriNetNode, PetriNetEdge>::new(),
            callgraph,
            alias,
            function_counter: HashMap::<DefId, (NodeIndex, NodeIndex)>::new(),
            function_vec: HashMap::<DefId, Vec<NodeIndex>>::new(),
            locks_counter: HashMap::<LockGuardId, NodeIndex>::new(),
            lock_info: HashMap::default(),
            condvars: HashMap::<CondVarId, NodeIndex>::new(),
            api_spec,
            api_marks: HashMap::<String, HashSet<(NodeIndex, u8)>>::new(),
            atomic_places: HashMap::<AliasId, NodeIndex>::new(),
            atomic_order_maps: HashMap::<AliasId, AtomicOrdering>::new(),
            entry_exit: (NodeIndex::new(0), NodeIndex::new(0)),
            enable_block_collector,
            enable_atomic_collector,
            enbale_unsafe_collector,
            unsafe_places: HashMap::default(),
            channel_places: HashMap::default(),
        }
    }

    fn marking_api(&mut self) {
        // Match format DefId
        let func_map: HashMap<String, NodeIndex> = self
            .function_counter
            .iter()
            .map(|(def_id, (start_node, _))| {
                let func_name = format_name(*def_id);
                (func_name, *start_node)
            })
            .collect();

        for api_entry in &self.api_spec.apis {
            match api_entry {
                ApiEntry::Single(api_name) => {
                    if let Some(&start_node) = func_map.get(api_name) {
                        let mut mark = HashSet::new();
                        mark.insert((start_node, 1)); // Set initial token to 1
                        self.api_marks.insert(api_name.clone(), mark);
                        log::debug!("Added mark for single API: {}", api_name);
                    }
                }
                ApiEntry::Group(apis) => {
                    let mut group_mark = HashSet::new();
                    for api_name in apis {
                        if let Some(&start_node) = func_map.get(api_name) {
                            group_mark.insert((start_node, 1));
                        }
                    }
                    if !group_mark.is_empty() {
                        // Use the first API name in the group as the key
                        let group_key = format!("group_{}", apis.join("_"));
                        self.api_marks.insert(group_key, group_mark);
                        log::debug!("Added mark for API group: [{}]", apis.join(", "));
                    }
                }
            }
        }
    }

    pub fn construct_channel_resources(&mut self) {
        let mut channel_collector =
            ChannelCollector::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        channel_collector.analyze();
        channel_collector.to_json_pretty().unwrap();

        // Channels created through Tuple will not allocate memory resources
        // for (id, channel_info) in channel_collector.channel_tuples {
        //     let channel_id = format!("{:?}", id);
        //     let channel_place = Place::new_with_span(
        //         channel_id,
        //         1,
        //         PlaceType::Channel,
        //         format!("{:?}", channel_info.0.span),
        //     );
        //     let channel_node = self.net.add_node(PetriNetNode::P(channel_place));
        //     self.channel_places.insert(id, channel_node);
        // }

        let mut span_groups: HashMap<String, Vec<(ChannelId, ChannelInfo<'tcx>)>> = HashMap::new();

        // Collect all channel endpoints and group by span
        for (id, info) in channel_collector.channels {
            let key_string = format!("{:?}", info.span)
                .split(":")
                .take(2)
                .collect::<Vec<&str>>()
                .join("");
            span_groups.entry(key_string).or_default().push((id, info));
        }

        // Process paired channel endpoints
        for (i, (span, endpoints)) in span_groups.iter().enumerate() {
            if endpoints.len() == 2 {
                // Ensure there is a pair of sender and receiver
                let has_pair = endpoints
                    .iter()
                    .any(|(_, info)| info.endpoint_type == EndpointType::Sender)
                    && endpoints
                        .iter()
                        .any(|(_, info)| info.endpoint_type == EndpointType::Receiver);

                if has_pair {
                    let channel_id = format!("channel_{}", i);
                    let channel_place =
                        Place::new_indefinite(channel_id, 0, 100, PlaceType::Channel, span.clone());
                    let channel_node = self.net.add_node(PetriNetNode::P(channel_place));

                    for (id, _) in endpoints {
                        self.channel_places.insert(id.clone(), channel_node);
                    }

                    log::debug!(
                        "Created shared channel place for endpoints at span: {}",
                        span
                    );
                }
            }
        }
    }

    pub fn construct_atomic_resources(&mut self) {
        let mut atomic_collector =
            AtomicCollector::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        let atomic_vars = atomic_collector.analyze();

        // Output collected atomic information
        atomic_collector.to_json_pretty().unwrap();
        for (_, atomic_info) in atomic_vars {
            let atomic_type = atomic_info.var_type.clone();
            let alias_id = atomic_info.get_alias_id();
            if !atomic_type.starts_with("&") {
                let atomic_name = atomic_type.clone();
                let atomic_place = Place::new_with_span(
                    atomic_name,
                    1,
                    PlaceType::Atomic,
                    atomic_info.span.clone(),
                );
                let atomic_node = self.net.add_node(PetriNetNode::P(atomic_place));

                self.atomic_places.insert(alias_id, atomic_node);
            } else {
                log::debug!(
                    "Adding atomic ordering: {:?} -> {:?}",
                    alias_id,
                    atomic_info.operations[0].ordering
                );
                self.atomic_order_maps
                    .insert(alias_id, atomic_info.operations[0].ordering);
            }
        }
    }

    fn construct_unsafe_blocks(&mut self) {
        let unsafe_analyzer =
            UnsafeAnalyzer::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        let (unsafe_info, unsafe_data) = unsafe_analyzer.analyze();
        unsafe_info.iter().for_each(|(def_id, info)| {
            log::debug!(
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
        log::debug!("unsafe_data size: {:?}", unsafe_data.unsafe_places.len());

        let mut next_alias_id: u32 = 0;
        let mut alias_groups: HashMap<u32, Vec<(AliasId, String)>> = HashMap::new();
        let places_data: Vec<_> = unsafe_data
            .unsafe_places
            .iter()
            .map(|(local, info)| (*local, info.clone()))
            .collect();

        for i in 0..places_data.len() {
            let (local_i, info_i) = &places_data[i];

            if alias_groups
                .values()
                .any(|group| group.iter().any(|(l, _)| l == local_i))
            {
                continue;
            }

            let mut current_group = vec![(local_i.clone(), info_i.clone())];

            for j in i + 1..places_data.len() {
                let (local_j, info_j) = &places_data[j];
                match self.alias.borrow_mut().alias(*local_i, *local_j) {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                        current_group.push((local_j.clone(), info_j.clone()));
                    }
                    _ => {}
                }
            }

            if !current_group.is_empty() {
                alias_groups.insert(next_alias_id, current_group);
                next_alias_id += 1;
            }
        }

        // Create a place for each alias group
        for (_, group) in alias_groups {
            let unsafe_span = group[0].1.clone();
            let unsafe_local = group[0].0.clone();
            let unsafe_name = format!("{:?}", unsafe_local);

            let place = Place::new_with_span(unsafe_name, 1, PlaceType::Unsafe, unsafe_span);

            let node = self.net.add_node(PetriNetNode::P(place));
            self.unsafe_places.insert(unsafe_local, node);

            for (local, _) in group {
                self.unsafe_places.insert(local, node);
            }
        }
    }

    pub fn construct(&mut self /*alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>*/) {
        let start_time = Instant::now();
        let cons_config = NetConfig::new(
            self.enable_block_collector,
            self.enable_atomic_collector,
            self.enbale_unsafe_collector,
        );

        log::info!("Construct Function Start and End Places");
        self.construct_func();

        if !self.api_spec.apis.is_empty() {
            log::info!("Consturct API Markings");
            self.marking_api();
        }

        if self.enable_block_collector {
            self.construct_lock_with_dfs();
            log::info!("Collector Block Primitive!");
            self.construct_channel_resources();
            log::info!("Collector Channel Resources!");
        }

        if self.enable_atomic_collector {
            self.construct_atomic_resources();
            log::info!("Collector Atomic Variable!")
        }

        if self.enbale_unsafe_collector {
            self.construct_unsafe_blocks();
            log::info!("Collector Unsafe Blocks!");
        }

        // Initialize synchronization API regular expressions
        let key_api_regex = KeyApiRegex::new();
        // Set an id to record converted functions
        let mut visited_func_id = HashSet::<DefId>::new();
        for (node, caller) in self.callgraph.graph.node_references() {
            if self.tcx.is_mir_available(caller.instance().def_id())
                && format_name(caller.instance().def_id()).starts_with(&self.options.crate_name)
            {
                log::debug!(
                    "Current visitor function body: {:?}",
                    format_name(caller.instance().def_id())
                );
                if visited_func_id.contains(&caller.instance().def_id()) {
                    continue;
                }
                self.visitor_function_body(node, caller, &key_api_regex, &cons_config);
                visited_func_id.insert(caller.instance().def_id());
            }
        }

        log::info!("Visitor Function Body Complete!");

        // If CrateType is LIB, do not optimize to prevent initial markings from being changed
        if self.api_spec.apis.is_empty() && !self.options.test {
            self.reduce_state();
            log::info!("Merge long(>= 5) P-T chains");
        }
        //self.reduce_state_from(self.entry_node);

        // Verify network structure
        if let Err(err) = self.verify_and_clean() {
            log::error!("Petri net structure verification failed: {}", err);
            // Can choose to panic here or handle other errors
        }
        log::info!("Construct Petri Net Time: {:?}", start_time.elapsed());
    }

    pub fn visitor_function_body(
        &mut self,
        node: NodeIndex,
        caller: &CallGraphNode<'tcx>,
        key_api_regex: &KeyApiRegex,
        cons_config: &NetConfig,
        //alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>,
    ) {
        let body = self.tcx.optimized_mir(caller.instance().def_id());
        // let body = self.tcx.instance_mir(caller.instance().def);
        // Skip promoted src
        if body.source.promoted.is_some() {
            return;
        }
        let lock_infos = self.lock_info.clone();

        let mut func_body = BodyToPetriNet::new(
            node,
            caller.instance(),
            body,
            self.tcx,
            // self.param_env,
            &self.callgraph,
            &mut self.net,
            &mut self.alias,
            lock_infos,
            &self.function_counter,
            &self.locks_counter,
            // &mut self.thread_id_handler,
            // &mut self.handler_id,
            &self.condvars,
            &self.atomic_places,
            &self.atomic_order_maps,
            self.entry_exit,
            &self.unsafe_places,
            key_api_regex,
            cons_config,
            &self.channel_places,
        );
        func_body.translate();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        // If crate is BIN, need to find the main function
        match self.options.crate_type {
            OwnCrateType::Bin => self.construct_bin_funcs(),
            OwnCrateType::Lib => self.construct_lib_funcs(),
        }
    }

    fn construct_bin_funcs(&mut self) {
        let main_func = match self.tcx.entry_fn(()) {
            Some((main_func, _)) => main_func,
            None => {
                log::debug!("cargo pta need a entry point!");
                return;
            }
        };

        self.process_functions(|self_, func_id, func_name| {
            if func_id == main_func {
                let (start, end) = self_.create_function_places(func_name, true);
                self_.entry_exit = (start, end);
                (start, end)
            } else {
                self_.create_function_places(func_name, false)
            }
        });
    }

    fn construct_lib_funcs(&mut self) {
        log::info!("construct lib funcs");
        self.process_functions(|self_, _, func_name| {
            self_.create_function_places(func_name, false)
        });
    }

    fn process_functions<F>(&mut self, create_places: F)
    where
        F: Fn(&mut Self, DefId, String) -> (NodeIndex, NodeIndex),
    {
        for node_idx in self.callgraph.graph.node_indices() {
            let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
            let func_id = func_instance.instance().def_id();
            let func_name = format_name(func_id);
            if !func_name.starts_with(&self.options.crate_name)
                || self.function_counter.contains_key(&func_id)
                || func_name.contains("::deserialize")
                || func_name.contains("::serialize")
                || func_name.contains("::visit_seq")
                || func_name.contains("::visit_map")
            {
                continue;
            }

            let (start, end) = create_places(self, func_id, func_name);
            self.function_counter.insert(func_id, (start, end));
            self.function_vec.insert(func_id, vec![start]);
        }
    }

    fn create_function_places(
        &mut self,
        func_name: String,
        with_token: bool,
    ) -> (NodeIndex, NodeIndex) {
        let start = if with_token {
            Place::new(format!("{}_start", func_name), 1, PlaceType::FunctionStart)
        } else {
            Place::new_with_no_token(format!("{}_start", func_name), PlaceType::FunctionStart)
        };
        let end = Place::new_with_no_token(format!("{}_end", func_name), PlaceType::FunctionEnd);

        let start_id = self.net.add_node(PetriNetNode::P(start));
        let end_id = self.net.add_node(PetriNetNode::P(end));

        (start_id, end_id)
    }

    pub fn construct_lock_with_dfs(&mut self) {
        // Use new collection function
        let lockguards = self.collect_blocking_primitives();
        let mut info = FxHashMap::default();

        for (_, map) in lockguards.into_iter() {
            info.extend(map);
        }

        let mut union_find: HashMap<LockGuardId, LockGuardId> = HashMap::new();
        let lockid_vec: Vec<LockGuardId> = info.clone().into_keys().collect();

        for lock_id in &lockid_vec {
            union_find.insert(lock_id.clone(), lock_id.clone());
        }

        // Add debug output: show all alias relationships
        log::debug!("=== Detected alias relationships ===");
        for i in 0..lockid_vec.len() {
            for j in i + 1..lockid_vec.len() {
                match self
                    .alias
                    .borrow_mut()
                    .alias(lockid_vec[i].clone().into(), lockid_vec[j].clone().into())
                {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                        log::debug!(
                            "Lock {:?} and {:?} have alias relationship",
                            lockid_vec[i],
                            lockid_vec[j]
                        );
                        union(&mut union_find, &lockid_vec[i], &lockid_vec[j]);
                    }
                    _ => {}
                }
            }
        }

        // First group by root nodes
        let mut temp_groups: HashMap<LockGuardId, Vec<LockGuardId>> = HashMap::new();
        for lock_id in &lockid_vec {
            let root = find(&union_find, lock_id);
            temp_groups.entry(root).or_default().push(lock_id.clone());
        }

        // Add debug output: show grouping results
        println!("\n=== Lock grouping results ===");
        for (group_id, (root, group)) in temp_groups.iter().enumerate() {
            println!("Group {}: ", group_id);
            println!("  Root node: {:?}", root);
            println!("  Group members:");
            for lock in group {
                let lock_type = match &info[lock].lockguard_ty {
                    LockGuardTy::StdMutex(_) => "StdMutex",
                    LockGuardTy::ParkingLotMutex(_) => "ParkingLotMutex",
                    LockGuardTy::SpinMutex(_) => "SpinMutex",
                    _ => "RwLock",
                };
                println!("    - {:?} (Type: {})", lock, lock_type);
            }
        }

        // Convert grouping to required format and create corresponding Place nodes
        let mut group_id = 0;
        for group in temp_groups.values() {
            match &info[&group[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = format!("Mutex_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 1, PlaceType::Lock);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    log::debug!("Created Mutex node: {}", lock_name);
                    for lock in group {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("RwLock_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 10, PlaceType::Lock);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    log::debug!("Created RwLock node: {}", lock_name);
                    for lock in group {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
            group_id += 1;
        }
        log::info!("Found {} lock groups in total", group_id);
    }

    /// Simplify states in Petri net by merging simple paths to reduce network complexity
    ///
    /// Specific steps:
    /// 1. Find all nodes with in-degree and out-degree ≤1 as starting points
    /// 2. Starting from each starting point, search in both directions (forward and backward) to find mergeable paths
    /// 3. For each found path:
    ///    - Ensure the start and end points of the path are Place nodes
    ///    - If path length >3, create a new Transition node to replace intermediate nodes
    ///    - Keep Place nodes at both ends of the path unchanged, delete all intermediate nodes
    /// 4. Finally delete all nodes marked for removal uniformly
    ///
    /// This simplification can significantly reduce the size of the Petri net while maintaining its basic behavioral characteristics
    pub fn reduce_state(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut all_nodes_to_remove = Vec::new();
        // Find all nodes with in-degree and out-degree ≤1
        for node in self.net.node_indices() {
            let in_degree = self.net.edges_directed(node, Direction::Incoming).count();
            let out_degree = self.net.edges_directed(node, Direction::Outgoing).count();

            if in_degree <= 1 && out_degree <= 1 {
                queue.push_back(node);
            }
        }
        // TODO: Set new termination conditions to prevent unsafe operations from being merged
        while let Some(start) = queue.pop_front() {
            if visited.contains(&start) {
                continue;
            }

            // Start BFS from start to find a chain
            let mut chain = vec![start];
            let mut current = start;
            visited.insert(start);

            // Traverse in both directions
            for direction in &[Direction::Outgoing, Direction::Incoming] {
                current = start;
                loop {
                    let neighbors: Vec<_> =
                        self.net.neighbors_directed(current, *direction).collect();

                    if neighbors.len() != 1 {
                        break;
                    }

                    let next = neighbors[0];
                    let next_in_degree = self.net.edges_directed(next, Direction::Incoming).count();
                    let next_out_degree =
                        self.net.edges_directed(next, Direction::Outgoing).count();

                    if next_in_degree > 1 || next_out_degree > 1 || visited.contains(&next) {
                        break;
                    }

                    visited.insert(next);
                    if *direction == Direction::Outgoing {
                        chain.push(next);
                    } else {
                        chain.insert(0, next);
                    }
                    current = next;
                }
            }

            // Adjust chain to ensure start and end are both Places
            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[0]] {
                    chain.remove(0);
                }
            }
            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[chain.len() - 1]] {
                    chain.pop();
                    // assert_eq!(chain.len(), chain_len - 1);
                }
            }
            // Check if the adjusted chain length meets simplification conditions
            if chain.len() > 3 {
                // Ensure chain is not empty
                if chain.is_empty() {
                    continue;
                }
                let p1 = chain[0];
                let p2 = chain[chain.len() - 1];

                // Ensure both p1 and p2 are Places
                if let (PetriNetNode::P(_), PetriNetNode::P(_)) = (&self.net[p1], &self.net[p2]) {
                    // Create new Transition
                    let new_trans = Transition::new(
                        format!("merged_trans_{}_{}", p1.index(), p2.index()),
                        ControlType::Goto,
                    );
                    let new_trans_idx = self.net.add_node(PetriNetNode::T(new_trans));

                    // Add new edges
                    self.net
                        .add_edge(p1, new_trans_idx, PetriNetEdge { label: 1u8 });
                    self.net
                        .add_edge(new_trans_idx, p2, PetriNetEdge { label: 1u8 });

                    // Merge node information on the path into one line output
                    let path_info = chain[1..chain.len()]
                        .iter()
                        .map(|&node| match &self.net[node] {
                            PetriNetNode::P(place) => format!("P({})", place.name),
                            PetriNetNode::T(transition) => format!("T({})", transition.name),
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    log::debug!("Path: {}", path_info);
                    // Collect nodes to be deleted
                    all_nodes_to_remove.extend(chain[1..chain.len() - 1].iter().cloned());
                }
            }
        }
        // Delete nodes uniformly after loop ends
        if !all_nodes_to_remove.is_empty() {
            // Sort by index from large to small
            all_nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
            // Delete nodes
            for node in all_nodes_to_remove {
                self.net.remove_node(node);
            }
        }
    }

    /// Analyze and simplify paths from start node to end node, preserving paths connected to special nodes
    /// 1. Use DFS to collect all paths from start_node to end_node
    /// 2. Mark paths connected to special nodes as valid paths
    /// 3. Collect nodes to keep (nodes appearing in valid paths)
    /// 4. Delete nodes that only appear in invalid paths
    pub fn reduce_state_from(
        &mut self,
        start_node: NodeIndex,
        end_node: NodeIndex,
        special_nodes: &[NodeIndex],
    ) {
        // Store all paths from start to end
        let mut all_paths: Vec<Vec<NodeIndex>> = Vec::new();
        // Store valid paths (paths connected to special nodes)
        let mut valid_paths: HashSet<Vec<NodeIndex>> = HashSet::new();
        // Store the path currently being explored
        let mut current_path: Vec<NodeIndex> = vec![start_node];
        // Record visited nodes to avoid simple loops
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        // DFS collect all paths
        self.collect_paths(
            start_node,
            end_node,
            &mut all_paths,
            &mut current_path,
            &mut visited,
            special_nodes,
            &mut valid_paths,
        );

        // Collect all nodes to keep (nodes appearing in valid paths)
        let mut nodes_to_keep: HashSet<NodeIndex> = HashSet::new();
        for path in &valid_paths {
            nodes_to_keep.extend(path.iter().cloned());
        }

        // Collect all nodes that can be deleted (nodes appearing in invalid paths and not in valid paths)
        let mut nodes_to_remove: HashSet<NodeIndex> = HashSet::new();
        for path in &all_paths {
            if !valid_paths.contains(path) {
                for &node in path {
                    if !nodes_to_keep.contains(&node) && node != start_node && node != end_node {
                        nodes_to_remove.insert(node);
                    }
                }
            }
        }

        let mut nodes_to_remove: Vec<_> = nodes_to_remove.into_iter().collect();
        nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
        for node in nodes_to_remove {
            self.net.remove_node(node);
        }
    }

    /// Recursively collect all paths from start to end
    ///
    /// 1. If reaching the end, check if current path is connected to special nodes
    /// 2. If path is valid, add to valid_paths
    /// 3. Add current path to all_paths
    /// 4. Recursively explore all unvisited neighbor nodes
    /// 5. Remove visit mark when backtracking, allowing nodes to be revisited in other paths
    fn collect_paths(
        &self,
        current: NodeIndex,
        end: NodeIndex,
        all_paths: &mut Vec<Vec<NodeIndex>>,
        current_path: &mut Vec<NodeIndex>,
        visited: &mut HashSet<NodeIndex>,
        special_nodes: &[NodeIndex],
        valid_paths: &mut HashSet<Vec<NodeIndex>>,
    ) {
        if current == end {
            // Check if path is connected to special nodes
            let path_has_special_connection = current_path.iter().any(|&node| {
                self.net
                    .neighbors(node)
                    .any(|neighbor| special_nodes.contains(&neighbor))
            });

            if path_has_special_connection {
                valid_paths.insert(current_path.clone());
            }
            all_paths.push(current_path.clone());
            return;
        }

        visited.insert(current);

        for neighbor in self.net.neighbors_directed(current, Direction::Outgoing) {
            if !visited.contains(&neighbor) {
                current_path.push(neighbor);
                self.collect_paths(
                    neighbor,
                    end,
                    all_paths,
                    current_path,
                    visited,
                    special_nodes,
                    valid_paths,
                );
                current_path.pop();
            }
        }

        visited.remove(&current);
    }

    // Mapping JoinHandle To Thread DefId
    fn collect_blocking_primitives(&mut self) -> FxHashMap<InstanceId, LockGuardMap<'tcx>> {
        let mut lockguards = FxHashMap::default();
        let mut condvars = FxHashMap::default();

        // Traverse callgraph to collect information
        for (instance_id, node) in self.callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };

            // Only analyze local functions
            if !instance.def_id().is_local() {
                continue;
            }

            let body = self.tcx.instance_mir(instance.def);
            let mut collector = BlockingCollector::new(instance_id, instance, body, self.tcx);
            collector.analyze();

            // Collect lock information
            if !collector.lockguards.is_empty() {
                lockguards.insert(instance_id, collector.lockguards.clone());
                self.lock_info.extend(collector.lockguards);
            }

            // Collect condition variable information
            if !collector.condvars.is_empty() {
                condvars.insert(instance_id, collector.condvars);
            }
        }

        // Process condition variables
        if !condvars.is_empty() {
            for condvar_map in condvars.into_values() {
                for (condvar_id, span) in condvar_map {
                    let condvar_name = format!("Condvar:{}", span);
                    let condvar_p = Place::new(condvar_name, 1, PlaceType::CondVar);
                    let condvar_node = self.net.add_node(PetriNetNode::P(condvar_p));
                    self.condvars.insert(condvar_id, condvar_node);
                }
            }
        } else {
            log::debug!("Not Found Condvars In This Crate");
        }

        // Return collected lock information for subsequent processing
        lockguards
    }

    pub fn get_current_mark(&self) -> HashSet<(NodeIndex, u8)> {
        let mut current_mark = HashSet::<(NodeIndex, u8)>::new();
        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *place.tokens.borrow() > 0 {
                        current_mark.insert((node.clone(), *place.tokens.borrow() as u8));
                    }
                }
                PetriNetNode::T(_) => {
                    debug!("{}", "this error!");
                }
            }
        }
        current_mark
    }

    pub fn get_or_insert_node(&mut self, def_id: DefId) -> (NodeIndex, NodeIndex) {
        match self.function_counter.entry(def_id) {
            Entry::Occupied(node) => node.get().to_owned(),
            Entry::Vacant(v) => {
                let func_name = self.tcx.def_path_str(def_id);
                let func_start =
                    Place::new(format!("{}_start", func_name), 0, PlaceType::FunctionStart);
                let func_start_node_id = self.net.add_node(PetriNetNode::P(func_start));
                let func_end = Place::new(format!("{}_end", func_name), 0, PlaceType::FunctionEnd);
                let func_end_node_id = self.net.add_node(PetriNetNode::P(func_end));
                *v.insert((func_start_node_id, func_end_node_id))
            }
        }
    }

    pub fn save_petri_net_to_file(&self) {
        if self.options.dump_options.dump_petri_net {
            use petgraph::dot::{Config, Dot};
            let pn_dot = Dot::with_attr_getters(
                &self.net,
                &[Config::NodeNoLabel],
                &|_, _| "arrowhead = vee".to_string(),
                &|_, nr| {
                    let label = match &nr.1 {
                        PetriNetNode::P(place) => {
                            let tokens = *place.tokens.borrow();
                            format!(
                                "label = \"{}\\n(tokens: {})\", shape = circle",
                                place.name, tokens
                            )
                        }
                        PetriNetNode::T(transition) => {
                            format!("label = \"{}\", shape = box", transition.name)
                        }
                    };
                    label
                },
            );

            let mut file = std::fs::File::create(self.output_directory.join("graph.dot")).unwrap();
            let _ = file.write_all(format!("{:?}", pn_dot).as_bytes());
            log::info!(
                "Petri net saved to {}",
                self.output_directory.join("graph.dot").display()
            );
        }
    }

    /// Verify the structural correctness of the Petri net
    ///
    /// Check the following rules:
    /// 1. All predecessors and successors of Transition nodes must be Place nodes
    /// 2. All predecessors and successors of Place nodes must be Transition nodes
    /// 3. Place nodes can have no predecessors or successors
    ///
    /// Returns:
    /// - Ok(()) if the network structure is correct
    /// - Err(String) containing error description string
    pub fn verify_structure(&self) -> Result<Vec<NodeIndex>> {
        let mut transitions_to_remove = Vec::new();

        // Check structure and collect transitions to be deleted
        for node_idx in self.net.node_indices() {
            match &self.net[node_idx] {
                PetriNetNode::T(transition) => {
                    // Check transition successors
                    let successors: Vec<_> = self
                        .net
                        .neighbors_directed(node_idx, Direction::Outgoing)
                        .collect();

                    if successors.is_empty() {
                        transitions_to_remove.push(node_idx);
                        log::warn!("Found transition with no successors: {}", transition.name);
                        continue;
                    }

                    // Check if all predecessors and successors of transitions are places
                    for pred in self.net.neighbors_directed(node_idx, Direction::Incoming) {
                        if let PetriNetNode::T(_) = &self.net[pred] {
                            return Err(PetriNetError::InvalidTransitionConnection {
                                transition_name: transition.name.clone(),
                                connection_type: "predecessor",
                            }
                            .into());
                        }
                    }

                    for succ in &successors {
                        if let PetriNetNode::T(_) = &self.net[*succ] {
                            return Err(PetriNetError::InvalidTransitionConnection {
                                transition_name: transition.name.clone(),
                                connection_type: "successor",
                            }
                            .into());
                        }
                    }
                }
                PetriNetNode::P(place) => {
                    // Check if all predecessors and successors of places are transitions
                    for pred in self.net.neighbors_directed(node_idx, Direction::Incoming) {
                        if let PetriNetNode::P(_) = &self.net[pred] {
                            return Err(PetriNetError::InvalidPlaceConnection {
                                place_name: place.name.clone(),
                                connection_type: "predecessor",
                            }
                            .into());
                        }
                    }

                    for succ in self.net.neighbors_directed(node_idx, Direction::Outgoing) {
                        if let PetriNetNode::P(_) = &self.net[succ] {
                            return Err(PetriNetError::InvalidPlaceConnection {
                                place_name: place.name.clone(),
                                connection_type: "successor",
                            }
                            .into());
                        }
                    }
                }
            }
        }

        Ok(transitions_to_remove)
    }

    pub fn remove_invalid_transitions(&mut self, transitions_to_remove: Vec<NodeIndex>) {
        // Delete nodes from back to front to keep indices valid
        for transition_idx in transitions_to_remove.iter().rev() {
            let _ = if let PetriNetNode::T(t) = &self.net[*transition_idx] {
                t.name.clone()
            } else {
                String::from("Unknown")
            };

            self.net.remove_node(*transition_idx);
        }

        if !transitions_to_remove.is_empty() {
            log::debug!(
                "Removed {} transitions with no successors",
                transitions_to_remove.len()
            );
        }
    }

    pub fn verify_and_clean(&mut self) -> Result<()> {
        let transitions_to_remove = self.verify_structure()?;
        self.remove_invalid_transitions(transitions_to_remove);
        Ok(())
    }

    pub fn get_terminal_states(&self) -> Vec<(usize, u8)> {
        let mut terminal_states = Vec::new();
        terminal_states.push((self.entry_exit.1.index(), 1));
        for node_idx in self.net.node_indices() {
            if let Some(PetriNetNode::P(place)) = self.net.node_weight(node_idx) {
                match place.place_type {
                    PlaceType::FunctionStart | PlaceType::FunctionEnd | PlaceType::BasicBlock => {
                        continue;
                    }
                    _ => {
                        terminal_states.push((node_idx.index(), *place.tokens.borrow()));
                    }
                }
            }
        }
        terminal_states
    }

    /// Second step reduction: Remove non-sensitive paths while preserving shared nodes
    ///
    /// This function identifies sensitive paths (containing Unsafe operations, Lock operations, etc.)
    /// and removes non-sensitive paths, but preserves nodes that are shared between sensitive paths.
    ///
    /// Arguments:
    /// - start_node: Starting place (typically main function start)
    /// - end_node: Ending place (typically main function end)
    pub fn reduce_non_sensitive_paths(&mut self, start_node: NodeIndex, end_node: NodeIndex) {
        log::info!("Starting second-step reduction: removing non-sensitive paths");

        // Step 1: Find all paths from start to end
        let mut all_paths: Vec<Vec<NodeIndex>> = Vec::new();
        let mut current_path: Vec<NodeIndex> = vec![start_node];
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        self.collect_all_paths(
            start_node,
            end_node,
            &mut all_paths,
            &mut current_path,
            &mut visited,
        );

        log::debug!("Found {} total paths from start to end", all_paths.len());

        // Step 2: Classify paths as sensitive or non-sensitive
        let mut sensitive_paths: HashSet<Vec<NodeIndex>> = HashSet::new();
        let mut non_sensitive_paths: HashSet<Vec<NodeIndex>> = HashSet::new();

        for path in &all_paths {
            if self.is_sensitive_path(path) {
                sensitive_paths.insert(path.clone());
                log::debug!("Path marked as sensitive: {} nodes", path.len());
            } else {
                non_sensitive_paths.insert(path.clone());
                log::debug!("Path marked as non-sensitive: {} nodes", path.len());
            }
        }

        log::debug!(
            "Found {} sensitive paths and {} non-sensitive paths",
            sensitive_paths.len(),
            non_sensitive_paths.len()
        );

        // Step 3: Find shared nodes between sensitive paths
        let mut shared_nodes: HashSet<NodeIndex> = HashSet::new();
        let mut node_count: HashMap<NodeIndex, usize> = HashMap::new();

        // Count occurrences in sensitive paths
        for path in &sensitive_paths {
            for &node in path {
                *node_count.entry(node).or_insert(0) += 1;
            }
        }

        // Nodes that appear in multiple sensitive paths are shared
        for (node, count) in node_count {
            if count > 1 {
                shared_nodes.insert(node);
            }
        }

        // Step 4: Collect nodes to preserve
        let mut nodes_to_preserve: HashSet<NodeIndex> = HashSet::new();

        // Always preserve start and end nodes
        nodes_to_preserve.insert(start_node);
        nodes_to_preserve.insert(end_node);

        // Preserve all shared nodes
        nodes_to_preserve.extend(&shared_nodes);

        // Preserve all nodes from sensitive paths
        for path in &sensitive_paths {
            nodes_to_preserve.extend(path.iter());
        }

        // Step 5: Remove nodes that only belong to non-sensitive paths
        let mut nodes_to_remove: Vec<NodeIndex> = Vec::new();

        for path in &non_sensitive_paths {
            for &node in path {
                if !nodes_to_preserve.contains(&node) {
                    nodes_to_remove.push(node);
                }
            }
        }

        // Remove duplicates and sort by index (largest first) for safe removal
        nodes_to_remove.sort();
        nodes_to_remove.dedup();
        nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));

        log::info!(
            "Removing {} nodes from non-sensitive paths",
            nodes_to_remove.len()
        );
        for node in nodes_to_remove {
            self.net.remove_node(node);
        }
    }

    /// Check if a path contains sensitive operations
    ///
    /// A path is sensitive if it contains transitions with:
    /// - Unsafe operations (UnsafeRead, UnsafeWrite)
    /// - Lock operations (Lock, RwLockRead, RwLockWrite, Unlock, etc.)
    /// - Atomic operations (AtomicLoad, AtomicStore, AtomicCmpXchg)
    /// - Condition variable operations (Wait, Notify)
    /// - Thread operations (Spawn, Join)
    ///
    /// Non-sensitive paths only contain Function calls and basic control flow.
    /// Note: Channel operations are typically modeled as Function calls and get
    /// their sensitivity through connections to Channel resource places.
    fn is_sensitive_path(&self, path: &[NodeIndex]) -> bool {
        for &node in path {
            if let Some(PetriNetNode::T(transition)) = self.net.node_weight(node) {
                match &transition.transition_type {
                    // Unsafe operations are always sensitive
                    ControlType::UnsafeRead(_, _, _, _) | ControlType::UnsafeWrite(_, _, _, _) => {
                        return true;
                    }

                    // Check call types for sensitive operations
                    ControlType::Call(call_type) => {
                        match call_type {
                            // Lock operations
                            CallType::Lock(_)
                            | CallType::RwLockRead(_)
                            | CallType::RwLockWrite(_)
                            | CallType::Wait
                            | CallType::Notify(_) => {
                                return true;
                            }

                            // Atomic operations
                            CallType::AtomicLoad(_, _, _, _)
                            | CallType::AtomicStore(_, _, _, _)
                            | CallType::AtomicCmpXchg(_, _, _, _, _) => {
                                return true;
                            }

                            // Thread operations
                            CallType::Spawn(_) | CallType::Join(_) => {
                                return true;
                            }

                            // Regular function calls are not sensitive
                            CallType::Function => {
                                // Continue checking other transitions in the path
                            }
                        }
                    }

                    // Drop operations for locks are sensitive
                    ControlType::Drop(drop_type) => {
                        match drop_type {
                            DropType::Unlock(_)
                            | DropType::DropRead(_)
                            | DropType::DropWrite(_) => {
                                return true;
                            }
                            DropType::Basic => {
                                // Basic drops are not sensitive
                            }
                        }
                    }

                    // Basic control flow is not sensitive
                    ControlType::Start(_)
                    | ControlType::Goto
                    | ControlType::Switch
                    | ControlType::Return(_)
                    | ControlType::Assert => {
                        // Continue checking other transitions
                    }
                }
            }
        }

        // If no sensitive operations found, path is non-sensitive
        false
    }

    /// Helper function to collect all simple paths from start to end
    fn collect_all_paths(
        &self,
        current: NodeIndex,
        end: NodeIndex,
        all_paths: &mut Vec<Vec<NodeIndex>>,
        current_path: &mut Vec<NodeIndex>,
        visited: &mut HashSet<NodeIndex>,
    ) {
        if current == end {
            all_paths.push(current_path.clone());
            return;
        }

        visited.insert(current);

        for neighbor in self.net.neighbors_directed(current, Direction::Outgoing) {
            if !visited.contains(&neighbor) {
                current_path.push(neighbor);
                self.collect_all_paths(neighbor, end, all_paths, current_path, visited);
                current_path.pop();
            }
        }

        visited.remove(&current);
    }

    /// Third step reduction: Advanced optimizations
    ///
    /// This function implements several advanced reduction techniques:
    /// 1. Dead code elimination: Remove unreachable transitions
    /// 2. Equivalent place merging: Merge places with identical behavior
    /// 3. Redundant transition removal: Remove transitions that don't change system state
    /// 4. Structural invariant-based reduction: Use Petri net invariants for reduction
    pub fn reduce_advanced_optimizations(&mut self, resource_nodes: &[NodeIndex]) {
        log::info!("Starting third-step reduction: advanced optimizations");

        // 1. Dead code elimination
        self.eliminate_dead_code();

        // 2. Remove isolated nodes (not connected to anything)
        self.remove_isolated_nodes(resource_nodes);

        // 3. Merge equivalent places
        self.merge_equivalent_places();

        // 4. Remove redundant transitions
        self.remove_redundant_transitions();

        log::info!("Advanced optimization reduction completed");
    }

    /// Remove transitions that are never enabled (dead code)
    fn eliminate_dead_code(&mut self) {
        let mut reachable_transitions: HashSet<NodeIndex> = HashSet::new();
        let mut queue: VecDeque<NodeIndex> = VecDeque::new();

        // Start from all places with tokens
        for node_idx in self.net.node_indices() {
            if let Some(PetriNetNode::P(place)) = self.net.node_weight(node_idx) {
                if *place.tokens.borrow() > 0 {
                    queue.push_back(node_idx);
                }
            }
        }

        // BFS to find all reachable transitions
        let mut visited: HashSet<NodeIndex> = HashSet::new();
        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current);

            for neighbor in self.net.neighbors(current) {
                if !visited.contains(&neighbor) {
                    if let Some(PetriNetNode::T(_)) = self.net.node_weight(neighbor) {
                        reachable_transitions.insert(neighbor);
                    }
                    queue.push_back(neighbor);
                }
            }
        }

        // Remove unreachable transitions
        let mut to_remove: Vec<NodeIndex> = Vec::new();
        for node_idx in self.net.node_indices() {
            if let Some(PetriNetNode::T(_)) = self.net.node_weight(node_idx) {
                if !reachable_transitions.contains(&node_idx) {
                    to_remove.push(node_idx);
                }
            }
        }

        to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
        for node in to_remove {
            self.net.remove_node(node);
        }
    }

    /// Remove nodes that are completely isolated (no connections)
    fn remove_isolated_nodes(&mut self, resource_nodes: &[NodeIndex]) {
        let mut to_remove: Vec<NodeIndex> = Vec::new();

        for node_idx in self.net.node_indices() {
            let in_degree = self
                .net
                .edges_directed(node_idx, Direction::Incoming)
                .count();
            let out_degree = self
                .net
                .edges_directed(node_idx, Direction::Outgoing)
                .count();

            if in_degree == 0 && out_degree == 0 {
                // Don't remove resource nodes even if isolated
                if !resource_nodes.contains(&node_idx) {
                    // Don't remove entry/exit nodes
                    if node_idx != self.entry_exit.0 && node_idx != self.entry_exit.1 {
                        to_remove.push(node_idx);
                    }
                }
            }
        }

        to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
        for node in to_remove.iter() {
            self.net.remove_node(*node);
        }

        if !to_remove.is_empty() {
            log::debug!("Removed {} isolated nodes", to_remove.len());
        }
    }

    /// Merge places that have identical token behavior and connections
    fn merge_equivalent_places(&mut self) {
        // This is a complex optimization that would require careful analysis
        // of place semantics. For now, we implement a simple version.
        log::debug!("Equivalent place merging - placeholder for future implementation");
        // TODO: Implement sophisticated place equivalence analysis
    }

    /// Remove transitions that don't change the system state meaningfully
    fn remove_redundant_transitions(&mut self) {
        let mut to_remove: Vec<NodeIndex> = Vec::new();

        for node_idx in self.net.node_indices() {
            if let Some(PetriNetNode::T(transition)) = self.net.node_weight(node_idx) {
                // Check if this is a simple pass-through transition
                let incoming: Vec<_> = self
                    .net
                    .neighbors_directed(node_idx, Direction::Incoming)
                    .collect();
                let outgoing: Vec<_> = self
                    .net
                    .neighbors_directed(node_idx, Direction::Outgoing)
                    .collect();

                // If it's a simple Goto with one input and one output, it might be redundant
                if incoming.len() == 1 && outgoing.len() == 1 {
                    if let ControlType::Goto = transition.transition_type {
                        // Check if we can safely merge this transition
                        let input_place = incoming[0];
                        let output_place = outgoing[0];

                        // Only merge if both are simple places (not resource places)
                        if let (Some(PetriNetNode::P(in_place)), Some(PetriNetNode::P(out_place))) = (
                            self.net.node_weight(input_place),
                            self.net.node_weight(output_place),
                        ) {
                            match (&in_place.place_type, &out_place.place_type) {
                                (PlaceType::BasicBlock, PlaceType::BasicBlock) => {
                                    to_remove.push(node_idx);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Remove redundant transitions and merge their connected places
        for transition_idx in to_remove.iter() {
            // Get input and output places before removing the transition
            let incoming: Vec<_> = self
                .net
                .neighbors_directed(*transition_idx, Direction::Incoming)
                .collect();
            let outgoing: Vec<_> = self
                .net
                .neighbors_directed(*transition_idx, Direction::Outgoing)
                .collect();

            if incoming.len() == 1 && outgoing.len() == 1 {
                let input_place = incoming[0];
                let output_place = outgoing[0];

                // Transfer all edges from output_place to input_place
                let output_neighbors: Vec<_> = self
                    .net
                    .neighbors_directed(output_place, Direction::Outgoing)
                    .collect();
                for neighbor in output_neighbors {
                    if let Some(edge) = self.net.find_edge(output_place, neighbor) {
                        let edge_weight = self.net.edge_weight(edge).unwrap().clone();
                        self.net.add_edge(input_place, neighbor, edge_weight);
                    }
                }

                // Remove the transition and output place
                self.net.remove_node(*transition_idx);
                self.net.remove_node(output_place);
            }
        }

        if !to_remove.is_empty() {
            log::debug!("Removed {} redundant transitions", to_remove.len());
        }
    }

    /// Comprehensive three-step Petri net reduction
    ///
    /// This function applies all three reduction steps in sequence:
    /// 1. Basic path merging (existing reduce_state)
    /// 2. Resource-based path pruning
    /// 3. Advanced optimizations
    ///
    /// Arguments:
    /// - resource_nodes: Optional vector of resource nodes. If None, will auto-detect.
    pub fn reduce_comprehensive(&mut self, resource_nodes: Option<Vec<NodeIndex>>) {
        log::info!("Starting comprehensive three-step Petri net reduction");

        let resource_nodes = resource_nodes.unwrap_or_else(|| self.get_resource_nodes());
        log::info!(
            "Identified {} resource nodes for reduction",
            resource_nodes.len()
        );

        // Step 1: Basic path merging (merge simple linear chains)
        log::info!("Step 1: Basic path merging");
        self.reduce_state();

        // Step 2: Resource-based path pruning
        log::info!("Step 2: Resource-based path pruning");
        if self.entry_exit.0 != self.entry_exit.1 {
            self.reduce_non_sensitive_paths(self.entry_exit.0, self.entry_exit.1);
        }

        // Step 3: Advanced optimizations
        log::info!("Step 3: Advanced optimizations");
        self.reduce_advanced_optimizations(&resource_nodes);

        // Final cleanup and verification
        if let Err(err) = self.verify_and_clean() {
            log::warn!("Post-reduction verification found issues: {}", err);
        }

        log::info!("Comprehensive reduction completed");
    }

    /// Automatically detect resource nodes in the Petri net
    ///
    /// Resource nodes include:
    /// - Lock places (Mutex, RwLock)
    /// - Atomic variable places
    /// - Channel places
    /// - Condition variable places
    /// - Unsafe operation places
    pub fn get_resource_nodes(&self) -> Vec<NodeIndex> {
        let mut resource_nodes = Vec::new();

        for node_idx in self.net.node_indices() {
            if let Some(PetriNetNode::P(place)) = self.net.node_weight(node_idx) {
                match place.place_type {
                    PlaceType::Lock
                    | PlaceType::Atomic
                    | PlaceType::Channel
                    | PlaceType::CondVar
                    | PlaceType::Unsafe => {
                        resource_nodes.push(node_idx);
                    }
                    _ => {}
                }
            }
        }

        log::debug!("Auto-detected resource nodes: {:?}", resource_nodes);
        resource_nodes
    }

    /// Get reduction statistics
    ///
    /// Returns information about the current state of the Petri net
    /// for monitoring reduction effectiveness
    pub fn get_reduction_stats(&self) -> (usize, usize, usize) {
        let mut places = 0;
        let mut transitions = 0;
        let mut resource_places = 0;

        for node_idx in self.net.node_indices() {
            match self.net.node_weight(node_idx) {
                Some(PetriNetNode::P(place)) => {
                    places += 1;
                    match place.place_type {
                        PlaceType::Lock
                        | PlaceType::Atomic
                        | PlaceType::Channel
                        | PlaceType::CondVar
                        | PlaceType::Unsafe => {
                            resource_places += 1;
                        }
                        _ => {}
                    }
                }
                Some(PetriNetNode::T(_)) => {
                    transitions += 1;
                }
                _ => {}
            }
        }

        (places, transitions, resource_places)
    }
}
