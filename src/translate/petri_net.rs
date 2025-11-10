use crate::concurrency::atomic::AtomicCollector;
use crate::concurrency::channel::{ChannelCollector, ChannelInfo, EndpointType};
use crate::memory::pointsto::AliasId;
use crate::memory::unsafe_memory::UnsafeAnalyzer;
use crate::net::structure::PlaceType;
use crate::translate::structure::{FunctionRegistry, KeyApiRegex, ResourceRegistry};
use crate::util::format_name;
use crate::Options;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;

use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use crate::concurrency::blocking::{BlockingCollector, LockGuardId, LockGuardMap, LockGuardTy};
use crate::memory::pointsto::{AliasAnalysis, ApproximateAliasKind};
use crate::net::{Net, Place, PlaceId};
use crate::translate::mir_to_pn::BodyToPetriNet;

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
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    pub net: Net,
    callgraph: &'pn CallGraph<'tcx>,
    pub alias: RefCell<AliasAnalysis<'pn, 'tcx>>,
    functions: FunctionRegistry,
    lock_info: LockGuardMap<'tcx>,
    resources: ResourceRegistry,
    pub entry_exit: (PlaceId, PlaceId),
}

impl<'compilation, 'pn, 'tcx> PetriNet<'compilation, 'pn, 'tcx> {
    fn create_resource_place(
        &mut self,
        name: String,
        initial: u64,
        capacity: u64,
        span: String,
    ) -> PlaceId {
        let place = Place::new(name, initial, capacity, PlaceType::Resources, span);
        self.net.add_place(place)
    }

    pub fn new(
        options: &'compilation Options,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        callgraph: &'pn CallGraph<'tcx>,
    ) -> Self {
        let alias = RefCell::new(AliasAnalysis::new(tcx, &callgraph));
        Self {
            options,
            tcx,
            net: Net::empty(),
            callgraph,
            alias,
            functions: FunctionRegistry::new(),
            lock_info: HashMap::default(),
            resources: ResourceRegistry::new(),
            entry_exit: (PlaceId::new(0), PlaceId::new(0)),
        }
    }

    pub fn construct_channel_resources(&mut self) {
        let mut channel_collector =
            ChannelCollector::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        channel_collector.analyze();
        channel_collector.to_json_pretty().unwrap();

        let mut span_groups: HashMap<String, Vec<(AliasId, ChannelInfo<'tcx>)>> = HashMap::new();

        for (id, info) in channel_collector.channels {
            let key_string = format!("{:?}", info.span)
                .split(":")
                .take(2)
                .collect::<Vec<&str>>()
                .join("");
            span_groups
                .entry(key_string)
                .or_default()
                .push((AliasId::from(id), info));
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
                    let channel_node = self.create_resource_place(channel_id, 0, 100, span.clone());

                    for (id, _) in endpoints {
                        self.resources
                            .channel_places_mut()
                            .insert(*id, channel_node);
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

        if atomic_collector.atomic_vars.is_empty() {
            log::debug!("Not Found Atomic Variables In This Crate");
            return;
        }
        let atomic_vars = atomic_collector.analyze();

        atomic_collector.to_json_pretty().unwrap();
        for (_, atomic_info) in atomic_vars {
            let atomic_type = atomic_info.var_type.clone();
            let alias_id = atomic_info.get_alias_id();
            if !atomic_type.starts_with("&") {
                let atomic_name = atomic_type.clone();
                let atomic_node =
                    self.create_resource_place(atomic_name, 1, 1, atomic_info.span.clone());

                self.resources
                    .atomic_places_mut()
                    .insert(alias_id, atomic_node);
            } else {
                log::debug!(
                    "Adding atomic ordering: {:?} -> {:?}",
                    alias_id,
                    atomic_info.operations[0].ordering
                );
                self.resources
                    .atomic_orders_mut()
                    .insert(alias_id, atomic_info.operations[0].ordering);
            }
        }
    }

    fn construct_unsafe_blocks(&mut self) {
        let unsafe_analyzer =
            UnsafeAnalyzer::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        let (unsafe_info, unsafe_data) = unsafe_analyzer.analyze();
        if unsafe_info.is_empty() {
            log::debug!("Not Found Unsafe Blocks In This Crate");
            return;
        }

        // unsafe_info.iter().for_each(|(def_id, info)| {
        //     log::debug!(
        //         "{}:\n{}",
        //         format_name(*def_id),
        //         serde_json::to_string_pretty(&json!({
        //             "unsafe_fn": info.is_unsafe_fn,
        //             "unsafe_blocks": info.unsafe_blocks,
        //             "unsafe_places": info.unsafe_places
        //         }))
        //         .unwrap()
        //     )
        // });

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

            let place_id = self.create_resource_place(unsafe_name, 1, 1, unsafe_span);
            self.resources
                .unsafe_places_mut()
                .insert(unsafe_local, place_id);

            for (local, _) in group {
                self.resources.unsafe_places_mut().insert(local, place_id);
            }
        }
    }

    pub fn construct(&mut self /*alias_analysis: &'pn RefCell<AliasAnalysis<'pn, 'tcx>>*/) {
        let start_time = Instant::now();

        log::info!("Construct Function Start and End Places");
        self.construct_func();

        self.construct_lock_with_dfs();
        self.construct_channel_resources();
        self.construct_atomic_resources();
        self.construct_unsafe_blocks();

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
                self.visitor_function_body(node, caller, &key_api_regex);
                visited_func_id.insert(caller.instance().def_id());
            }
        }

        log::info!("Visitor Function Body Complete!");
        log::info!("Construct Petri Net Time: {:?}", start_time.elapsed());
    }

    pub fn visitor_function_body(
        &mut self,
        node: NodeIndex,
        caller: &CallGraphNode<'tcx>,
        key_api_regex: &KeyApiRegex,
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
            &self.functions,
            &self.resources,
            self.entry_exit,
            key_api_regex,
        );
        func_body.translate();
    }

    pub fn construct_func(&mut self) {
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

    fn process_functions<F>(&mut self, create_places: F)
    where
        F: Fn(&mut Self, DefId, String) -> (PlaceId, PlaceId),
    {
        for node_idx in self.callgraph.graph.node_indices() {
            let func_instance = self.callgraph.graph.node_weight(node_idx).unwrap();
            let func_id = func_instance.instance().def_id();
            let func_name = format_name(func_id);
            if !func_name.starts_with(&self.options.crate_name)
                || self.functions.contains(&func_id)
                || Self::should_ignore_function(&func_name)
            {
                continue;
            }

            let (start, end) = create_places(self, func_id, func_name);
            self.functions.insert(func_id, start, end);
        }
    }

    fn should_ignore_function(func_name: &str) -> bool {
        const IGNORED_SUBSTRINGS: &[&str] = &[
            "::serialize",
            "::serialize_",
            "::deserialize",
            "::deserialize_",
            "::serde",
            "::serde_json",
            "::serde_yaml",
            "::serde_with",
            "::__serde",
            "::__private",
            "::Serializer::",
            "::Deserializer::",
            "::Serialize::",
            "::Deserialize::",
            "::visit_",
            "::Visitor::visit",
            "::fmt::",
            "::Debug::fmt",
            "::core::fmt",
            "::alloc::fmt",
            "::tests::",
            "::test::",
            "::bench",
        ];

        IGNORED_SUBSTRINGS
            .iter()
            .any(|pattern| func_name.contains(pattern))
    }

    fn create_function_places(
        &mut self,
        func_name: String,
        with_token: bool,
    ) -> (PlaceId, PlaceId) {
        let start = if with_token {
            Place::new(
                format!("{}_start", func_name),
                1,
                1,
                PlaceType::FunctionStart,
                String::default(),
            )
        } else {
            Place::new(
                format!("{}_start", func_name),
                0,
                1,
                PlaceType::FunctionStart,
                String::default(),
            )
        };
        let end = Place::new(
            format!("{}_end", func_name),
            0,
            1,
            PlaceType::FunctionEnd,
            String::default(),
        );

        let start_id = self.net.add_place(start);
        let end_id = self.net.add_place(end);

        (start_id, end_id)
    }

    pub fn construct_lock_with_dfs(&mut self) {
        let lockguards = self.collect_blocking_primitives();
        if lockguards.is_empty() {
            log::debug!("Not Found Lockguards In This Crate");
            return;
        }

        let mut info = FxHashMap::default();

        for (_, map) in lockguards.into_iter() {
            info.extend(map);
        }

        let mut union_find: HashMap<LockGuardId, LockGuardId> = HashMap::new();
        let lockid_vec: Vec<LockGuardId> = info.clone().into_keys().collect();

        for lock_id in &lockid_vec {
            union_find.insert(lock_id.clone(), lock_id.clone());
        }

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

        let mut group_id = 0;
        for group in temp_groups.values() {
            match &info[&group[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = format!("Mutex_{}", group_id);
                    let lock_node =
                        self.create_resource_place(lock_name.clone(), 1, 1, String::default());
                    log::debug!("创建 Mutex 节点: {}", lock_name);
                    for lock in group {
                        let alias_id = lock.get_alias_id();
                        self.resources.locks_mut().insert(alias_id, lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("RwLock_{}", group_id);
                    let lock_node =
                        self.create_resource_place(lock_name.clone(), 10, 10, String::default());
                    log::debug!("创建 RwLock 节点: {}", lock_name);
                    for lock in group {
                        let alias_id = lock.get_alias_id();
                        self.resources.locks_mut().insert(alias_id, lock_node);
                    }
                }
            }
            group_id += 1;
        }
        log::debug!("总共发现 {} 个锁组", group_id);
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
                    let condvar_node =
                        self.create_resource_place(condvar_name, 1, 1, String::default());
                    let condvar_alias = condvar_id.get_alias_id();
                    self.resources
                        .condvars_mut()
                        .insert(condvar_alias, condvar_node);
                }
            }
        } else {
            log::debug!("Not Found Condvars In This Crate");
        }

        lockguards
    }

    pub fn get_or_insert_node(&mut self, def_id: DefId) -> (PlaceId, PlaceId) {
        self.functions.get_or_insert(def_id, || {
            let func_name = self.tcx.def_path_str(def_id);
            let func_start = Place::new(
                format!("{}_start", func_name),
                0,
                1,
                PlaceType::FunctionStart,
                String::default(),
            );
            let func_start_node_id = self.net.add_place(func_start);
            let func_end = Place::new(
                format!("{}_end", func_name),
                0,
                1,
                PlaceType::FunctionEnd,
                String::default(),
            );
            let func_end_node_id = self.net.add_place(func_end);
            (func_start_node_id, func_end_node_id)
        })
    }

    pub fn unsafe_places(&self) -> &HashMap<AliasId, PlaceId> {
        self.resources.unsafe_places()
    }

    pub fn channel_places(&self) -> &HashMap<AliasId, PlaceId> {
        self.resources.channel_places()
    }
}
