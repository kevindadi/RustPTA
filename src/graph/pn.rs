use crate::concurrency::atomic::{AtomicCollector, AtomicOrdering};
use crate::concurrency::channel::{ChannelCollector, ChannelId, ChannelInfo, EndpointType};
use crate::graph::net_structure::{ControlType, KeyApiRegex, NetConfig, Transition};
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

fn find(union_find: &HashMap<LockGuardId, LockGuardId>, x: &LockGuardId) -> LockGuardId {
    let mut current = x;
    while union_find[current] != *current {
        current = &union_find[current];
    }
    current.clone()
}

fn union(union_find: &mut HashMap<LockGuardId, LockGuardId>, x: &LockGuardId, y: &LockGuardId) {
    let root_x = find(union_find, x);
    let root_y = find(union_find, y);
    if root_x != root_y {
        union_find.insert(root_y, root_x);
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
                        mark.insert((start_node, 1));
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

        let mut span_groups: HashMap<String, Vec<(ChannelId, ChannelInfo<'tcx>)>> = HashMap::new();

        for (id, info) in channel_collector.channels {
            let key_string = format!("{:?}", info.span)
                .split(":")
                .take(2)
                .collect::<Vec<&str>>()
                .join("");
            span_groups.entry(key_string).or_default().push((id, info));
        }

        for (i, (span, endpoints)) in span_groups.iter().enumerate() {
            if endpoints.len() == 2 {
                let has_pair = endpoints
                    .iter()
                    .any(|(_, info)| info.endpoint_type == EndpointType::Sender)
                    && endpoints
                        .iter()
                        .any(|(_, info)| info.endpoint_type == EndpointType::Receiver);

                if has_pair {
                    let channel_id = format!("channel_{}", i);
                    let channel_place = Place::new_indefinite(
                        channel_id,
                        0,
                        100,
                        PlaceType::Resources,
                        span.clone(),
                    );
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

        atomic_collector.to_json_pretty().unwrap();
        for (_, atomic_info) in atomic_vars {
            let atomic_type = atomic_info.var_type.clone();
            let alias_id = atomic_info.get_alias_id();
            if !atomic_type.starts_with("&") {
                let atomic_name = atomic_type.clone();
                let atomic_place = Place::new_with_span(
                    atomic_name,
                    1,
                    PlaceType::Resources,
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

        for (_, group) in alias_groups {
            let unsafe_span = group[0].1.clone();
            let unsafe_local = group[0].0.clone();
            let unsafe_name = format!("{:?}", unsafe_local);

            let place = Place::new_with_span(unsafe_name, 1, PlaceType::Resources, unsafe_span);

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

        let key_api_regex = KeyApiRegex::new();

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

        if self.api_spec.apis.is_empty() && !self.options.test {
            self.reduce_state();
            log::info!("Merge long(>= 5) P-T chains");
        }

        if let Err(err) = self.verify_and_clean() {
            log::error!("Petri net structure verification failed: {}", err);
        }
        log::info!("Construct Petri Net Time: {:?}", start_time.elapsed());
    }

    pub fn visitor_function_body(
        &mut self,
        node: NodeIndex,
        caller: &CallGraphNode<'tcx>,
        key_api_regex: &KeyApiRegex,
        cons_config: &NetConfig,
    ) {
        let body = self.tcx.optimized_mir(caller.instance().def_id());

        if body.source.promoted.is_some() {
            return;
        }
        let lock_infos = self.lock_info.clone();

        let mut func_body = BodyToPetriNet::new(
            node,
            caller.instance(),
            body,
            self.tcx,
            &self.callgraph,
            &mut self.net,
            &mut self.alias,
            lock_infos,
            &self.function_counter,
            &self.locks_counter,
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

    pub fn construct_func(&mut self) {
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

        log::debug!("=== 检测到的别名关系 ===");
        for i in 0..lockid_vec.len() {
            for j in i + 1..lockid_vec.len() {
                match self
                    .alias
                    .borrow_mut()
                    .alias(lockid_vec[i].clone().into(), lockid_vec[j].clone().into())
                {
                    ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                        log::debug!("锁 {:?} 和 {:?} 存在别名关系", lockid_vec[i], lockid_vec[j]);
                        union(&mut union_find, &lockid_vec[i], &lockid_vec[j]);
                    }
                    _ => {}
                }
            }
        }

        let mut temp_groups: HashMap<LockGuardId, Vec<LockGuardId>> = HashMap::new();
        for lock_id in &lockid_vec {
            let root = find(&union_find, lock_id);
            temp_groups.entry(root).or_default().push(lock_id.clone());
        }

        println!("\n=== 锁的分组结果 ===");
        for (group_id, (root, group)) in temp_groups.iter().enumerate() {
            println!("组 {}: ", group_id);
            println!("  根节点: {:?}", root);
            println!("  组内成员:");
            for lock in group {
                let lock_type = match &info[lock].lockguard_ty {
                    LockGuardTy::StdMutex(_) => "StdMutex",
                    LockGuardTy::ParkingLotMutex(_) => "ParkingLotMutex",
                    LockGuardTy::SpinMutex(_) => "SpinMutex",
                    _ => "RwLock",
                };
                println!("    - {:?} (类型: {})", lock, lock_type);
            }
        }

        let mut group_id = 0;
        for group in temp_groups.values() {
            match &info[&group[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = format!("Mutex_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 1, PlaceType::Resources);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    log::debug!("创建 Mutex 节点: {}", lock_name);
                    for lock in group {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("RwLock_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 10, PlaceType::Resources);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    log::debug!("创建 RwLock 节点: {}", lock_name);
                    for lock in group {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
            }
            group_id += 1;
        }
        log::info!("总共发现 {} 个锁组", group_id);
    }

    pub fn reduce_state(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut all_nodes_to_remove = Vec::new();

        for node in self.net.node_indices() {
            let in_degree = self.net.edges_directed(node, Direction::Incoming).count();
            let out_degree = self.net.edges_directed(node, Direction::Outgoing).count();

            if in_degree <= 1 && out_degree <= 1 {
                queue.push_back(node);
            }
        }

        while let Some(start) = queue.pop_front() {
            if visited.contains(&start) {
                continue;
            }

            let mut chain = vec![start];
            let mut current = start;
            visited.insert(start);

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

            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[0]] {
                    chain.remove(0);
                }
            }
            if !chain.is_empty() {
                if let PetriNetNode::T(_) = &self.net[chain[chain.len() - 1]] {
                    chain.pop();
                }
            }

            if chain.len() > 3 {
                if chain.is_empty() {
                    continue;
                }
                let p1 = chain[0];
                let p2 = chain[chain.len() - 1];

                if let (PetriNetNode::P(_), PetriNetNode::P(_)) = (&self.net[p1], &self.net[p2]) {
                    let new_trans = Transition::new(
                        format!("merged_trans_{}_{}", p1.index(), p2.index()),
                        ControlType::Goto,
                    );
                    let new_trans_idx = self.net.add_node(PetriNetNode::T(new_trans));

                    self.net
                        .add_edge(p1, new_trans_idx, PetriNetEdge { label: 1u8 });
                    self.net
                        .add_edge(new_trans_idx, p2, PetriNetEdge { label: 1u8 });

                    let path_info = chain[1..chain.len()]
                        .iter()
                        .map(|&node| match &self.net[node] {
                            PetriNetNode::P(place) => format!("P({})", place.name),
                            PetriNetNode::T(transition) => format!("T({})", transition.name),
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    log::debug!("Path: {}", path_info);

                    all_nodes_to_remove.extend(chain[1..chain.len() - 1].iter().cloned());
                }
            }
        }

        if !all_nodes_to_remove.is_empty() {
            all_nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));

            for node in all_nodes_to_remove {
                self.net.remove_node(node);
            }
        }
    }

    pub fn reduce_state_from(
        &mut self,
        start_node: NodeIndex,
        end_node: NodeIndex,
        special_nodes: &[NodeIndex],
    ) {
        let mut all_paths: Vec<Vec<NodeIndex>> = Vec::new();

        let mut valid_paths: HashSet<Vec<NodeIndex>> = HashSet::new();

        let mut current_path: Vec<NodeIndex> = vec![start_node];

        let mut visited: HashSet<NodeIndex> = HashSet::new();

        self.collect_paths(
            start_node,
            end_node,
            &mut all_paths,
            &mut current_path,
            &mut visited,
            special_nodes,
            &mut valid_paths,
        );

        let mut nodes_to_keep: HashSet<NodeIndex> = HashSet::new();
        for path in &valid_paths {
            nodes_to_keep.extend(path.iter().cloned());
        }

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

    fn collect_blocking_primitives(&mut self) -> FxHashMap<InstanceId, LockGuardMap<'tcx>> {
        let mut lockguards = FxHashMap::default();
        let mut condvars = FxHashMap::default();

        for (instance_id, node) in self.callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };

            if !instance.def_id().is_local() {
                continue;
            }

            let body = self.tcx.instance_mir(instance.def);
            let mut collector = BlockingCollector::new(instance_id, instance, body, self.tcx);
            collector.analyze();

            if !collector.lockguards.is_empty() {
                lockguards.insert(instance_id, collector.lockguards.clone());
                self.lock_info.extend(collector.lockguards);
            }

            if !collector.condvars.is_empty() {
                condvars.insert(instance_id, collector.condvars);
            }
        }

        if !condvars.is_empty() {
            for condvar_map in condvars.into_values() {
                for (condvar_id, span) in condvar_map {
                    let condvar_name = format!("Condvar:{}", span);
                    let condvar_p = Place::new(condvar_name, 1, PlaceType::Resources);
                    let condvar_node = self.net.add_node(PetriNetNode::P(condvar_p));
                    self.condvars.insert(condvar_id, condvar_node);
                }
            }
        } else {
            log::debug!("Not Found Condvars In This Crate");
        }

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

    pub fn verify_structure(&self) -> Result<Vec<NodeIndex>> {
        let mut transitions_to_remove = Vec::new();

        for node_idx in self.net.node_indices() {
            match &self.net[node_idx] {
                PetriNetNode::T(transition) => {
                    let successors: Vec<_> = self
                        .net
                        .neighbors_directed(node_idx, Direction::Outgoing)
                        .collect();

                    if successors.is_empty() {
                        transitions_to_remove.push(node_idx);
                        log::warn!("Found transition with no successors: {}", transition.name);
                        continue;
                    }

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

    pub fn reduce_resource_free_cycles(&mut self) {
        let resource_places: HashSet<NodeIndex> = self
            .net
            .node_indices()
            .filter(|&node| {
                if let PetriNetNode::P(place) = &self.net[node] {
                    place.place_type == PlaceType::Resources
                } else {
                    false
                }
            })
            .collect();

        if resource_places.is_empty() {
            log::debug!("No resource places found in the net");
            return;
        }

        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        for start_node in self.net.node_indices() {
            self.find_cycles(start_node, start_node, &mut visited, &mut path, &mut cycles);
        }

        let mut cycles_to_remove = Vec::new();
        for cycle in cycles {
            if self.is_resource_free_cycle(&cycle, &resource_places) {
                cycles_to_remove.push(cycle);
            }
        }

        let mut nodes_to_remove = HashSet::new();
        for cycle in cycles_to_remove {
            nodes_to_remove.extend(cycle);
        }

        let mut nodes: Vec<_> = nodes_to_remove.into_iter().collect();
        nodes.sort_by(|a, b| b.index().cmp(&a.index()));

        let removed_count = nodes.len();
        for node in nodes {
            self.net.remove_node(node);
        }

        log::debug!("Removed {} nodes from resource-free cycles", removed_count);
    }

    fn find_cycles(
        &self,
        current: NodeIndex,
        start: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        path: &mut Vec<NodeIndex>,
        cycles: &mut Vec<Vec<NodeIndex>>,
    ) {
        if !path.is_empty() && current == start {
            cycles.push(path.clone());
            return;
        }

        if visited.contains(&current) {
            return;
        }

        visited.insert(current);
        path.push(current);

        for neighbor in self.net.neighbors(current) {
            self.find_cycles(neighbor, start, visited, path, cycles);
        }

        path.pop();
        visited.remove(&current);
    }

    fn is_resource_free_cycle(
        &self,
        cycle: &[NodeIndex],
        resource_places: &HashSet<NodeIndex>,
    ) -> bool {
        for &node in cycle {
            if let PetriNetNode::T(_) = &self.net[node] {
                let neighbors: HashSet<NodeIndex> = self.net.neighbors(node).collect();
                if neighbors.intersection(resource_places).next().is_some() {
                    return false;
                }
            }
        }
        true
    }
}
