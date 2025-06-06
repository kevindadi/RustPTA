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

/// 分支合并机会的描述结构
#[derive(Debug, Clone)]
pub struct BranchMergeOpportunity {
    /// 分支点节点（可选，如果所有路径从同一点开始）
    pub branch_point: Option<NodeIndex>,
    /// 汇聚到同一敏感节点的路径索引
    pub convergent_paths: Vec<usize>,
    /// 可以合并的路径段
    pub mergeable_segment: Vec<NodeIndex>,
}

/// 资源节点的标识符，统一不同类型的资源ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceId {
    Lock(LockGuardId),
    Atomic(AliasId),
    Channel(ChannelId),
    Unsafe(AliasId),
    CondVar(CondVarId),
}

/// 资源节点信息
#[derive(Debug, Clone)]
pub struct ResourceInfo<'tcx> {
    /// 资源节点在图中的索引
    pub node_index: NodeIndex,
    /// 资源类型
    pub resource_type: ResourceType,
    /// 资源名称
    pub name: String,
    /// 资源的额外信息（如原子操作顺序等）
    pub metadata: ResourceMetadata<'tcx>,
}

/// 资源类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// 互斥锁 (Mutex)
    Mutex,
    /// 读写锁 (RwLock)
    RwLock,
    /// 原子变量
    Atomic,
    /// 通道 (Channel)
    Channel,
    /// 不安全内存操作
    Unsafe,
    /// 条件变量
    CondVar,
}

/// 资源的元数据信息
#[derive(Debug, Clone)]
pub enum ResourceMetadata<'tcx> {
    /// 锁相关的元数据
    Lock {
        lock_type: LockGuardTy<'tcx>,
        span: String,
    },
    /// 原子变量相关的元数据
    Atomic {
        ordering: Option<AtomicOrdering>,
        var_type: String,
        span: String,
    },
    /// 通道相关的元数据
    Channel {
        capacity: Option<usize>,
        span: String,
    },
    /// 不安全操作相关的元数据
    Unsafe {
        operation_type: String,
        span: String,
    },
    /// 条件变量相关的元数据
    CondVar { span: String },
}

/// 统一管理所有资源节点的结构
#[derive(Debug, Default)]
pub struct ResourceManager<'tcx> {
    /// 资源映射表：资源ID -> 资源信息
    resources: HashMap<ResourceId, ResourceInfo<'tcx>>,
    /// 按类型分组的资源索引
    type_groups: HashMap<ResourceType, Vec<ResourceId>>,
}

impl<'tcx> ResourceManager<'tcx> {
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            type_groups: HashMap::new(),
        }
    }

    /// 添加资源节点
    pub fn add_resource(&mut self, id: ResourceId, info: ResourceInfo<'tcx>) {
        // 添加到类型分组
        self.type_groups
            .entry(info.resource_type.clone())
            .or_default()
            .push(id.clone());

        // 添加到资源映射表
        self.resources.insert(id, info);
    }

    /// 获取指定资源的信息
    pub fn get_resource(&self, id: &ResourceId) -> Option<&ResourceInfo<'tcx>> {
        self.resources.get(id)
    }

    /// 获取指定类型的所有资源
    pub fn get_resources_by_type(&self, resource_type: &ResourceType) -> Vec<&ResourceInfo<'tcx>> {
        self.type_groups
            .get(resource_type)
            .map(|ids| ids.iter().filter_map(|id| self.resources.get(id)).collect())
            .unwrap_or_default()
    }

    /// 获取所有资源节点的NodeIndex
    pub fn get_all_node_indices(&self) -> Vec<NodeIndex> {
        self.resources
            .values()
            .map(|info| info.node_index)
            .collect()
    }

    /// 获取指定类型资源的NodeIndex
    pub fn get_node_indices_by_type(&self, resource_type: &ResourceType) -> Vec<NodeIndex> {
        self.get_resources_by_type(resource_type)
            .into_iter()
            .map(|info| info.node_index)
            .collect()
    }

    /// 根据NodeIndex查找资源信息
    pub fn find_resource_by_node(
        &self,
        node_index: NodeIndex,
    ) -> Option<(&ResourceId, &ResourceInfo<'tcx>)> {
        self.resources
            .iter()
            .find(|(_, info)| info.node_index == node_index)
    }

    /// 获取资源统计信息
    pub fn get_statistics(&self) -> HashMap<ResourceType, usize> {
        let mut stats = HashMap::new();
        for resource_type in &[
            ResourceType::Mutex,
            ResourceType::RwLock,
            ResourceType::Atomic,
            ResourceType::Channel,
            ResourceType::Unsafe,
            ResourceType::CondVar,
        ] {
            let count = self
                .type_groups
                .get(resource_type)
                .map(|v| v.len())
                .unwrap_or(0);
            stats.insert(resource_type.clone(), count);
        }
        stats
    }

    /// 移除资源
    pub fn remove_resource(&mut self, id: &ResourceId) -> Option<ResourceInfo<'tcx>> {
        if let Some(info) = self.resources.remove(id) {
            // 从类型分组中移除
            if let Some(group) = self.type_groups.get_mut(&info.resource_type) {
                group.retain(|rid| rid != id);
            }
            Some(info)
        } else {
            None
        }
    }

    /// 判断是否包含指定资源
    pub fn contains_resource(&self, id: &ResourceId) -> bool {
        self.resources.contains_key(id)
    }

    /// 获取所有资源ID
    pub fn get_all_resource_ids(&self) -> Vec<&ResourceId> {
        self.resources.keys().collect()
    }

    /// 清空所有资源
    pub fn clear(&mut self) {
        self.resources.clear();
        self.type_groups.clear();
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

    // 统一的资源管理器
    pub resource_manager: ResourceManager<'tcx>,

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
            resource_manager: ResourceManager::new(),
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
            self.reduce_non_sensitive_paths(self.entry_exit.0, self.entry_exit.1);
        }
        //self.reduce_state_from(self.entry_node);

        // Verify network structure
        if let Err(err) = self.verify_and_clean() {
            log::error!("Petri net structure verification failed: {}", err);
            // Can choose to panic here or handle other errors
        }
        let construction_duration = start_time.elapsed();
        let (final_places, final_transitions, final_edges) = self.get_network_size();

        log::info!("=== Petri网构建完成 ===");
        log::info!(
            "最终网络大小: {} places, {} transitions, {} edges",
            final_places,
            final_transitions,
            final_edges
        );
        log::info!("构建总时间: {:?}", construction_duration);
        log::info!("========================\n");
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
        let start_time = Instant::now();
        let (initial_places, initial_transitions, initial_edges) = self.get_network_size();

        log::info!("=== 开始基本路径合并缩减 ===");
        log::info!(
            "初始网络大小: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );
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

        let duration = start_time.elapsed();
        let (final_places, final_transitions, final_edges) = self.get_network_size();

        log::info!("基本路径合并缩减完成:");
        log::info!(
            "  缩减前: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );
        log::info!(
            "  缩减后: {} places, {} transitions, {} edges",
            final_places,
            final_transitions,
            final_edges
        );
        log::info!(
            "  节点减少: {} places, {} transitions, {} edges",
            initial_places.saturating_sub(final_places),
            initial_transitions.saturating_sub(final_transitions),
            initial_edges.saturating_sub(final_edges)
        );
        log::info!("  缩减时间: {:?}", duration);
        log::info!("=== 基本路径合并缩减结束 ===\n");
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
    /// This function implements intelligent path analysis that:
    /// 1. Identifies sensitive nodes (not just sensitive paths)
    /// 2. For each sensitive node, finds all paths leading to it
    /// 3. Identifies branch points where multiple paths converge to the same sensitive node
    /// 4. Merges non-sensitive branch paths that lead to the same sensitive node
    ///
    /// Arguments:
    /// - start_node: Starting place (typically main function start)
    /// - end_node: Ending place (typically main function end)
    pub fn reduce_non_sensitive_paths(&mut self, start_node: NodeIndex, end_node: NodeIndex) {
        let start_time = Instant::now();
        let (initial_places, initial_transitions, initial_edges) = self.get_network_size();

        log::info!("=== 开始智能敏感路径分析缩减 ===");
        log::info!(
            "初始网络大小: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );

        // Step 1: Identify all sensitive nodes in the network
        let sensitive_nodes = self.find_sensitive_nodes();
        log::info!("Found {} sensitive nodes", sensitive_nodes.len());

        // Step 2: Find all paths from start to each sensitive node
        let mut sensitive_node_paths: HashMap<NodeIndex, Vec<Vec<NodeIndex>>> = HashMap::new();

        for &sensitive_node in &sensitive_nodes {
            let mut paths = Vec::new();
            let mut current_path = vec![start_node];
            let mut visited = HashSet::new();

            self.collect_all_paths(
                start_node,
                sensitive_node,
                &mut paths,
                &mut current_path,
                &mut visited,
            );

            if !paths.is_empty() {
                sensitive_node_paths.insert(sensitive_node, paths);
            }
        }

        // Step 3: For each sensitive node, analyze its incoming paths for branch merging opportunities
        let mut nodes_to_preserve: HashSet<NodeIndex> = HashSet::new();

        // Always preserve start and end nodes
        nodes_to_preserve.insert(start_node);
        nodes_to_preserve.insert(end_node);

        // Preserve all sensitive nodes
        nodes_to_preserve.extend(&sensitive_nodes);

        for (sensitive_node, paths) in &sensitive_node_paths {
            log::debug!(
                "Analyzing {} paths to sensitive node {:?}",
                paths.len(),
                sensitive_node
            );

            // Find merge opportunities for this sensitive node
            let (nodes_to_keep, merge_opportunities) =
                self.analyze_branch_merge_opportunities(paths);
            nodes_to_preserve.extend(nodes_to_keep);

            // Apply merge optimizations
            self.apply_branch_merges(merge_opportunities);
        }

        // Step 4: Find paths from sensitive nodes to end node and preserve necessary nodes
        for &sensitive_node in &sensitive_nodes {
            let mut paths = Vec::new();
            let mut current_path = vec![sensitive_node];
            let mut visited = HashSet::new();

            self.collect_all_paths(
                sensitive_node,
                end_node,
                &mut paths,
                &mut current_path,
                &mut visited,
            );

            // Preserve nodes in paths from sensitive nodes to end
            for path in &paths {
                for &node in path {
                    nodes_to_preserve.insert(node);
                }
            }
        }

        // Step 5: Remove nodes that are not preserved
        let mut nodes_to_remove: Vec<NodeIndex> = Vec::new();

        for node_idx in self.net.node_indices() {
            if !nodes_to_preserve.contains(&node_idx) {
                nodes_to_remove.push(node_idx);
            }
        }

        // Remove duplicates and sort by index (largest first) for safe removal
        nodes_to_remove.sort();
        nodes_to_remove.dedup();
        nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));

        log::info!("移除 {} 个非必要节点", nodes_to_remove.len());
        for node in nodes_to_remove {
            self.net.remove_node(node);
        }

        let duration = start_time.elapsed();
        let (final_places, final_transitions, final_edges) = self.get_network_size();

        log::info!("智能敏感路径分析缩减完成:");
        log::info!(
            "  缩减前: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );
        log::info!(
            "  缩减后: {} places, {} transitions, {} edges",
            final_places,
            final_transitions,
            final_edges
        );
        log::info!(
            "  节点减少: {} places, {} transitions, {} edges",
            initial_places.saturating_sub(final_places),
            initial_transitions.saturating_sub(final_transitions),
            initial_edges.saturating_sub(final_edges)
        );
        log::info!("  缩减时间: {:?}", duration);
        log::info!("=== 智能敏感路径分析缩减结束 ===\n");
    }

    /// Find all sensitive nodes in the Petri net
    ///
    /// Sensitive nodes include:
    /// - Transitions with sensitive operations (unsafe, lock, atomic, etc.)
    /// - Resource places (lock, atomic, channel, condvar, unsafe places)
    fn find_sensitive_nodes(&self) -> HashSet<NodeIndex> {
        let mut sensitive_nodes = HashSet::new();

        for node_idx in self.net.node_indices() {
            match self.net.node_weight(node_idx) {
                Some(PetriNetNode::T(transition)) => {
                    if self.is_sensitive_transition(transition) {
                        sensitive_nodes.insert(node_idx);
                    }
                }
                Some(PetriNetNode::P(place)) => {
                    if self.is_sensitive_place(place) {
                        sensitive_nodes.insert(node_idx);
                    }
                }
                _ => {}
            }
        }

        sensitive_nodes
    }

    /// Check if a transition contains sensitive operations
    fn is_sensitive_transition(&self, transition: &Transition) -> bool {
        match &transition.transition_type {
            // Unsafe operations are always sensitive
            ControlType::UnsafeRead(_, _, _, _) | ControlType::UnsafeWrite(_, _, _, _) => true,

            // Check call types for sensitive operations
            ControlType::Call(call_type) => match call_type {
                // Lock operations
                CallType::Lock(_)
                | CallType::RwLockRead(_)
                | CallType::RwLockWrite(_)
                | CallType::Wait
                | CallType::Notify(_) => true,

                // Atomic operations
                CallType::AtomicLoad(_, _, _, _)
                | CallType::AtomicStore(_, _, _, _)
                | CallType::AtomicCmpXchg(_, _, _, _, _) => true,

                // Thread operations
                CallType::Spawn(_) | CallType::Join(_) => true,

                // Regular function calls are not sensitive
                CallType::Function => false,
            },

            // Drop operations for locks are sensitive
            ControlType::Drop(drop_type) => match drop_type {
                DropType::Unlock(_) | DropType::DropRead(_) | DropType::DropWrite(_) => true,
                DropType::Basic => false,
            },

            // Basic control flow is not sensitive
            ControlType::Start(_)
            | ControlType::Goto
            | ControlType::Switch
            | ControlType::Return(_)
            | ControlType::Assert => false,
        }
    }

    /// Check if a place is a sensitive resource place
    fn is_sensitive_place(&self, place: &Place) -> bool {
        matches!(
            place.place_type,
            PlaceType::Lock
                | PlaceType::Atomic
                | PlaceType::Channel
                | PlaceType::CondVar
                | PlaceType::Unsafe
        )
    }

    /// Analyze branch merge opportunities for paths leading to a sensitive node
    ///
    /// Returns:
    /// - HashSet of nodes that must be preserved
    /// - Vec of merge opportunities (branch points and their convergent paths)
    fn analyze_branch_merge_opportunities(
        &self,
        paths: &[Vec<NodeIndex>],
    ) -> (HashSet<NodeIndex>, Vec<BranchMergeOpportunity>) {
        let mut nodes_to_preserve = HashSet::new();
        let mut merge_opportunities = Vec::new();

        if paths.len() < 2 {
            // Need at least 2 paths to have merge opportunities
            if let Some(path) = paths.first() {
                nodes_to_preserve.extend(path.iter());
            }
            return (nodes_to_preserve, merge_opportunities);
        }

        // Find common prefix (branch point)
        let mut common_prefix_len = 0;
        let min_path_len = paths.iter().map(|p| p.len()).min().unwrap_or(0);

        for i in 0..min_path_len {
            let first_node = paths[0][i];
            if paths.iter().all(|path| path[i] == first_node) {
                common_prefix_len = i + 1;
            } else {
                break;
            }
        }

        // Preserve common prefix
        if common_prefix_len > 0 {
            for i in 0..common_prefix_len {
                nodes_to_preserve.insert(paths[0][i]);
            }
        }

        // Analyze branch segments after common prefix
        if common_prefix_len < min_path_len {
            let branch_point = if common_prefix_len > 0 {
                Some(paths[0][common_prefix_len - 1])
            } else {
                None
            };

            // Group paths by their branch segments
            let mut branch_segments: HashMap<Vec<NodeIndex>, Vec<usize>> = HashMap::new();

            for (path_idx, path) in paths.iter().enumerate() {
                let segment = path[common_prefix_len..].to_vec();
                branch_segments.entry(segment).or_default().push(path_idx);
            }

            // Identify merge opportunities
            for (segment, path_indices) in branch_segments {
                if path_indices.len() > 1 && !self.segment_contains_sensitive_operations(&segment) {
                    // This segment appears in multiple paths and contains no sensitive operations
                    // It's a candidate for merging
                    merge_opportunities.push(BranchMergeOpportunity {
                        branch_point,
                        convergent_paths: path_indices,
                        mergeable_segment: segment.clone(),
                    });
                }

                // Preserve the segment nodes
                nodes_to_preserve.extend(segment.iter());
            }
        }

        // Preserve the target sensitive node (last node in each path)
        for path in paths {
            if let Some(&target_node) = path.last() {
                nodes_to_preserve.insert(target_node);
            }
        }

        (nodes_to_preserve, merge_opportunities)
    }

    /// Check if a path segment contains sensitive operations
    fn segment_contains_sensitive_operations(&self, segment: &[NodeIndex]) -> bool {
        for &node in segment {
            match self.net.node_weight(node) {
                Some(PetriNetNode::T(transition)) => {
                    if self.is_sensitive_transition(transition) {
                        return true;
                    }
                }
                Some(PetriNetNode::P(place)) => {
                    if self.is_sensitive_place(place) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Apply branch merging optimizations
    ///
    /// For now, this is a placeholder for more sophisticated merging logic
    /// In a full implementation, this would:
    /// 1. Create new merged transitions to replace branch segments
    /// 2. Reconnect the graph to bypass redundant paths
    /// 3. Remove now-unused nodes
    fn apply_branch_merges(&mut self, merge_opportunities: Vec<BranchMergeOpportunity>) {
        for opportunity in merge_opportunities {
            log::debug!(
                "Found merge opportunity: {} convergent paths at branch point {:?}",
                opportunity.convergent_paths.len(),
                opportunity.branch_point
            );

            // For now, just log the opportunity
            // TODO: Implement actual merging logic
            // This would involve:
            // 1. Creating a new transition to represent the merged behavior
            // 2. Connecting the branch point to the new transition
            // 3. Connecting the new transition to the convergence point
            // 4. Removing the original branch segments
        }
    }

    /// Check if a path contains sensitive operations (legacy method for compatibility)
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
        self.segment_contains_sensitive_operations(path)
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
        let start_time = Instant::now();
        let (initial_places, initial_transitions, initial_edges) = self.get_network_size();

        log::info!("=== 开始高级优化缩减 ===");
        log::info!(
            "初始网络大小: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );

        // 1. Dead code elimination
        self.eliminate_dead_code();

        // 2. Remove isolated nodes (not connected to anything)
        self.remove_isolated_nodes(resource_nodes);

        // 3. Merge equivalent places
        self.merge_equivalent_places();

        // 4. Remove redundant transitions
        self.remove_redundant_transitions();

        let duration = start_time.elapsed();
        let (final_places, final_transitions, final_edges) = self.get_network_size();

        log::info!("高级优化缩减完成:");
        log::info!(
            "  缩减前: {} places, {} transitions, {} edges",
            initial_places,
            initial_transitions,
            initial_edges
        );
        log::info!(
            "  缩减后: {} places, {} transitions, {} edges",
            final_places,
            final_transitions,
            final_edges
        );
        log::info!(
            "  节点减少: {} places, {} transitions, {} edges",
            initial_places.saturating_sub(final_places),
            initial_transitions.saturating_sub(final_transitions),
            initial_edges.saturating_sub(final_edges)
        );
        log::info!("  缩减时间: {:?}", duration);
        log::info!("=== 高级优化缩减结束 ===\n");
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
        let total_start_time = Instant::now();
        let (total_initial_places, total_initial_transitions, total_initial_edges) =
            self.get_network_size();

        log::info!("=== 开始综合三步Petri网缩减 ===");
        log::info!(
            "总体初始网络大小: {} places, {} transitions, {} edges",
            total_initial_places,
            total_initial_transitions,
            total_initial_edges
        );

        let resource_nodes = resource_nodes.unwrap_or_else(|| self.get_resource_nodes());
        log::info!("识别到 {} 个资源节点用于缩减", resource_nodes.len());

        // Step 1: Basic path merging (merge simple linear chains)
        log::info!("第一步: 基本路径合并");
        self.reduce_state();

        // Step 2: Resource-based path pruning
        log::info!("第二步: 基于资源的路径修剪");
        if self.entry_exit.0 != self.entry_exit.1 {
            self.reduce_non_sensitive_paths(self.entry_exit.0, self.entry_exit.1);
        }

        // Step 3: Advanced optimizations
        log::info!("第三步: 高级优化");
        self.reduce_advanced_optimizations(&resource_nodes);

        // Final cleanup and verification
        if let Err(err) = self.verify_and_clean() {
            log::warn!("缩减后验证发现问题: {}", err);
        }

        let total_duration = total_start_time.elapsed();
        let (total_final_places, total_final_transitions, total_final_edges) =
            self.get_network_size();

        log::info!("=== 综合缩减总结 ===");
        log::info!(
            "总体缩减前: {} places, {} transitions, {} edges",
            total_initial_places,
            total_initial_transitions,
            total_initial_edges
        );
        log::info!(
            "总体缩减后: {} places, {} transitions, {} edges",
            total_final_places,
            total_final_transitions,
            total_final_edges
        );
        log::info!(
            "总体节点减少: {} places, {} transitions, {} edges",
            total_initial_places.saturating_sub(total_final_places),
            total_initial_transitions.saturating_sub(total_final_transitions),
            total_initial_edges.saturating_sub(total_final_edges)
        );

        let place_reduction_rate = if total_initial_places > 0 {
            ((total_initial_places.saturating_sub(total_final_places)) as f64
                / total_initial_places as f64)
                * 100.0
        } else {
            0.0
        };
        let transition_reduction_rate = if total_initial_transitions > 0 {
            ((total_initial_transitions.saturating_sub(total_final_transitions)) as f64
                / total_initial_transitions as f64)
                * 100.0
        } else {
            0.0
        };
        let edge_reduction_rate = if total_initial_edges > 0 {
            ((total_initial_edges.saturating_sub(total_final_edges)) as f64
                / total_initial_edges as f64)
                * 100.0
        } else {
            0.0
        };

        log::info!(
            "缩减率: {:.2}% places, {:.2}% transitions, {:.2}% edges",
            place_reduction_rate,
            transition_reduction_rate,
            edge_reduction_rate
        );
        log::info!("总缩减时间: {:?}", total_duration);
        log::info!("=== 综合缩减完成 ===\n");
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

    /// Get network size statistics
    ///
    /// Returns (places_count, transitions_count, edges_count)
    pub fn get_network_size(&self) -> (usize, usize, usize) {
        let mut places = 0;
        let mut transitions = 0;

        for node_idx in self.net.node_indices() {
            match self.net.node_weight(node_idx) {
                Some(PetriNetNode::P(_)) => {
                    places += 1;
                }
                Some(PetriNetNode::T(_)) => {
                    transitions += 1;
                }
                _ => {}
            }
        }

        let edges = self.net.edge_count();
        (places, transitions, edges)
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

    pub fn add_lock_resource(
        &mut self,
        lock_id: LockGuardId,
        node_index: NodeIndex,
        name: String,
        lock_type: LockGuardTy<'tcx>,
        span: String,
    ) {
        let resource_type = match lock_type {
            LockGuardTy::StdMutex(_)
            | LockGuardTy::ParkingLotMutex(_)
            | LockGuardTy::SpinMutex(_) => ResourceType::Mutex,
            _ => ResourceType::RwLock,
        };

        let resource_info = ResourceInfo {
            node_index,
            resource_type,
            name,
            metadata: ResourceMetadata::Lock { lock_type, span },
        };

        self.resource_manager
            .add_resource(ResourceId::Lock(lock_id), resource_info);
    }

    pub fn add_atomic_resource(
        &mut self,
        alias_id: AliasId,
        node_index: NodeIndex,
        name: String,
        var_type: String,
        span: String,
        ordering: Option<AtomicOrdering>,
    ) {
        let resource_info = ResourceInfo {
            node_index,
            resource_type: ResourceType::Atomic,
            name,
            metadata: ResourceMetadata::Atomic {
                ordering,
                var_type,
                span,
            },
        };

        self.resource_manager
            .add_resource(ResourceId::Atomic(alias_id), resource_info);
    }

    pub fn add_channel_resource(
        &mut self,
        channel_id: ChannelId,
        node_index: NodeIndex,
        name: String,
        span: String,
        capacity: Option<usize>,
    ) {
        let resource_info = ResourceInfo {
            node_index,
            resource_type: ResourceType::Channel,
            name,
            metadata: ResourceMetadata::Channel { capacity, span },
        };

        self.resource_manager
            .add_resource(ResourceId::Channel(channel_id), resource_info);
    }

    pub fn add_unsafe_resource(
        &mut self,
        alias_id: AliasId,
        node_index: NodeIndex,
        name: String,
        operation_type: String,
        span: String,
    ) {
        let resource_info = ResourceInfo {
            node_index,
            resource_type: ResourceType::Unsafe,
            name,
            metadata: ResourceMetadata::Unsafe {
                operation_type,
                span,
            },
        };

        self.resource_manager
            .add_resource(ResourceId::Unsafe(alias_id), resource_info);
    }

    pub fn add_condvar_resource(
        &mut self,
        condvar_id: CondVarId,
        node_index: NodeIndex,
        name: String,
        span: String,
    ) {
        let resource_info = ResourceInfo {
            node_index,
            resource_type: ResourceType::CondVar,
            name,
            metadata: ResourceMetadata::CondVar { span },
        };

        self.resource_manager
            .add_resource(ResourceId::CondVar(condvar_id), resource_info);
    }

    pub fn get_all_resource_nodes(&self) -> Vec<NodeIndex> {
        self.resource_manager.get_all_node_indices()
    }

    pub fn get_resource_nodes_by_type(&self, resource_type: &ResourceType) -> Vec<NodeIndex> {
        self.resource_manager
            .get_node_indices_by_type(resource_type)
    }

    pub fn print_resource_statistics(&self) {
        let stats = self.resource_manager.get_statistics();
        log::info!("=== Resource Statistics ===");
        for (resource_type, count) in stats {
            log::info!("{:?}: {} resources", resource_type, count);
        }
        log::info!(
            "Total resources: {}",
            self.resource_manager.get_all_node_indices().len()
        );
    }
}
