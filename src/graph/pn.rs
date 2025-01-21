use crate::concurrency::atomic::AtomicCollector;
use crate::concurrency::atomic::AtomicOrdering;
use crate::memory::pointsto::AliasId;
use crate::memory::unsafe_memory::UnsafeAnalyzer;
use crate::options::OwnCrateType;
use crate::utils::format_name;
use crate::utils::ApiEntry;
use crate::utils::ApiSpec;
use crate::Options;
use anyhow::Result;
use log::debug;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeReferences;
use petgraph::Direction;
use petgraph::Graph;
use regex::Regex;
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use serde_json::json;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use thiserror::Error;

use super::callgraph::{CallGraph, CallGraphNode, InstanceId};
use super::mir_pn::BodyToPetriNet;
use crate::concurrency::blocking::{
    BlockingCollector, CondVarId, LockGuardId, LockGuardMap, LockGuardTy,
};
use crate::memory::pointsto::{AliasAnalysis, ApproximateAliasKind};

#[derive(Debug, Clone)]
pub enum Shape {
    Circle,
    Box,
}

#[derive(Debug, Clone)]
pub struct Place {
    pub name: String,
    // pub tokens: RefCell<usize>,
    pub tokens: Arc<RwLock<u8>>,
    pub capacity: u8,
    pub span: String,
    pub place_type: PlaceType,
}

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum PlaceType {
    Unsafe,
    Atomic,
    Lock,
    CondVar,
    FunctionStart,
    FunctionEnd,
    BasicBlock,
}

impl Place {
    pub fn new(name: String, token: u8, place_type: PlaceType) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(token)),
            capacity: token,
            span: String::new(),
            place_type,
        }
    }

    pub fn new_with_span(name: String, token: u8, place_type: PlaceType, span: String) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(token)),
            capacity: 1u8,
            span,
            place_type,
        }
    }

    pub fn new_with_no_token(name: String, place_type: PlaceType) -> Self {
        Self {
            name,
            tokens: Arc::new(RwLock::new(0)),
            capacity: 1u8,
            span: String::new(),
            place_type,
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct Transition {
    pub name: String,
    pub weight: u32,
    shape: Shape,
    pub transition_type: ControlType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlType {
    // 基本控制结构
    Start(InstanceId),
    Goto,               // 直接跳转
    Switch,             // 条件分支
    Return(InstanceId), // 函数返回
    Drop(DropType),     // 资源释放
    Assert,

    UnsafeRead(NodeIndex, String, usize),
    UnsafeWrite(NodeIndex, String, usize),

    // 函数调用
    Call(CallType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallType {
    // 同步原语调用
    Lock(NodeIndex),
    RwLockRead(NodeIndex),
    RwLockWrite(NodeIndex),
    Notify(NodeIndex),
    Wait,

    // 原子操作
    AtomicLoad(AliasId, AtomicOrdering, String, InstanceId),
    AtomicStore(AliasId, AtomicOrdering, String, InstanceId),
    AtomicCmpXchg(AliasId, AtomicOrdering, AtomicOrdering, String, InstanceId),

    // 线程操作-后续reduce网会改变NodeIndex
    // 资源最先创建不因网结构改变
    Spawn(String),
    Join(String),

    // 普通函数调用
    Function,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DropType {
    Unlock(NodeIndex),
    DropRead(NodeIndex),
    DropWrite(NodeIndex),
    Basic,
}

impl Transition {
    pub fn new(name: String, transition_type: ControlType) -> Self {
        Self {
            name,
            transition_type,
            weight: 1,
            shape: Shape::Box,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PetriNetNode {
    P(Place),
    T(Transition),
}

impl std::fmt::Display for PetriNetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PetriNetNode::P(place) => write!(f, "{}", place.name),
            PetriNetNode::T(transition) => write!(f, "{}", transition.name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PetriNetEdge {
    pub label: u8,
}

impl std::fmt::Display for PetriNetEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    marks: HashMap<NodeIndex, usize>, // NodeIndex represents the place, usize represents token count
}

impl Hash for Marking {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (key, value) in &self.marks {
            key.hash(state);
            value.hash(state);
        }
    }
}

#[derive(Error, Debug)]
pub enum PetriNetError {
    #[error("Invalid Petri net structure: Transition '{transition_name}' has a Transition {connection_type}")]
    InvalidTransitionConnection {
        transition_name: String,
        connection_type: &'static str, // "predecessor" 或 "successor"
    },

    #[error("Invalid Petri net structure: Place '{place_name}' has a Place {connection_type}")]
    InvalidPlaceConnection {
        place_name: String,
        connection_type: &'static str,
    },
}

fn find(union_find: &HashMap<LockGuardId, LockGuardId>, x: &LockGuardId) -> LockGuardId {
    let mut current = x;
    while union_find[current] != *current {
        current = &union_find[current];
    }
    current.clone()
}

// 并查集的合并函数
fn union(union_find: &mut HashMap<LockGuardId, LockGuardId>, x: &LockGuardId, y: &LockGuardId) {
    let root_x = find(union_find, x);
    let root_y = find(union_find, y);
    if root_x != root_y {
        union_find.insert(root_y, root_x);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CollectorType {
    Blocking,
    Atomic,
    Unsafe,
}

impl CollectorType {
    pub fn is_enabled(&self, config: &NetConfig) -> bool {
        match self {
            CollectorType::Blocking => config.enable_blocking,
            CollectorType::Atomic => config.enable_atomic,
            CollectorType::Unsafe => config.enable_unsafe,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetConfig {
    pub enable_blocking: bool,
    pub enable_atomic: bool,
    pub enable_unsafe: bool,
}

impl NetConfig {
    pub fn new(enable_blocking: bool, enable_atomic: bool, enable_unsafe: bool) -> Self {
        Self {
            enable_blocking,
            enable_atomic,
            enable_unsafe,
        }
    }

    pub fn all_enabled() -> Self {
        Self::new(true, true, true)
    }

    pub fn none_enabled() -> Self {
        Self::new(false, false, false)
    }
}

pub struct KeyApiRegex {
    // Std::thread
    pub thread_join: Regex,
    pub scope_spwan: Regex,
    pub scope_join: Regex,
    pub condvar_notify: Regex,
    pub condvar_wait: Regex,

    pub atomic_load: Regex,
    pub atomic_store: Regex,
}

impl KeyApiRegex {
    fn new() -> Self {
        Self {
            thread_join: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            scope_spwan: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            scope_join: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            condvar_notify: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::notify").unwrap(),
            condvar_wait: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::wait").unwrap(),
            atomic_load: Regex::new(r"atomic[:a-zA-Z0-9]*::load").unwrap(),
            atomic_store: Regex::new(r"atomic[:a-zA-Z0-9]*::store").unwrap(),
        }
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
        }
    }

    fn marking_api(&mut self) {
        // 匹配format DefId
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
                        mark.insert((start_node, 1)); // 设置初始 token 为 1
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
                        // 使用组中的第一个 API 名称作为键
                        let group_key = format!("group_{}", apis.join("_"));
                        self.api_marks.insert(group_key, group_mark);
                        log::debug!("Added mark for API group: [{}]", apis.join(", "));
                    }
                }
            }
        }
    }

    pub fn construct_atomic_resources(&mut self) {
        let mut atomic_collector =
            AtomicCollector::new(self.tcx, self.callgraph, self.options.crate_name.clone());
        let atomic_vars = atomic_collector.analyze();

        // 输出收集到的atomic信息
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

        // 为每个别名组创建数据库所
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
        }

        if self.enable_atomic_collector {
            self.construct_atomic_resources();
            log::info!("Collector Atomic Variable!")
        }

        if self.enbale_unsafe_collector {
            self.construct_unsafe_blocks();
            log::info!("Collector Unsafe Blocks!");
        }

        // 初始化同步 API 的正则表达式
        let key_api_regex = KeyApiRegex::new();
        // 设置一个id,记录已经转换的函数
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

        // 如果CrateType是LIB，不优化以防初始标识被改变
        if self.api_spec.apis.is_empty() && !self.options.test {
            self.reduce_state();
            log::info!("Merge long(>= 3) P-T chains");
        }
        //self.reduce_state_from(self.entry_node);

        // 验证网络结构
        if let Err(err) = self.verify_and_clean() {
            log::error!("Petri net structure verification failed: {}", err);
            // 可以选择在这里panic或者进行其他错误处理
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
        );
        func_body.translate();
    }

    // Construct Function Start and End Place by callgraph
    pub fn construct_func(&mut self) {
        // 如果crate是BIN，则需要找到main函数
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
        // 使用新的收集函数
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

        // 添加调试输出：显示所有的别名关系
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

        // 先按根节点分组
        let mut temp_groups: HashMap<LockGuardId, Vec<LockGuardId>> = HashMap::new();
        for lock_id in &lockid_vec {
            let root = find(&union_find, lock_id);
            temp_groups.entry(root).or_default().push(lock_id.clone());
        }

        // 添加调试输出：显示分组结果
        log::debug!("\n=== 锁的分组结果 ===");
        for (group_id, (root, group)) in temp_groups.iter().enumerate() {
            log::debug!("组 {}: ", group_id);
            log::debug!("  根节点: {:?}", root);
            log::debug!("  组内成员:");
            for lock in group {
                let lock_type = match &info[lock].lockguard_ty {
                    LockGuardTy::StdMutex(_) => "StdMutex",
                    LockGuardTy::ParkingLotMutex(_) => "ParkingLotMutex",
                    LockGuardTy::SpinMutex(_) => "SpinMutex",
                    _ => "RwLock",
                };
                log::debug!("    - {:?} (类型: {})", lock, lock_type);
            }
        }

        // 将分组转换为所需的格式并创建对应的Place节点
        let mut group_id = 0;
        for group in temp_groups.values() {
            match &info[&group[0]].lockguard_ty {
                LockGuardTy::StdMutex(_)
                | LockGuardTy::ParkingLotMutex(_)
                | LockGuardTy::SpinMutex(_) => {
                    let lock_name = format!("Mutex_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 1, PlaceType::Lock);
                    let lock_node = self.net.add_node(PetriNetNode::P(lock_p));
                    log::debug!("创建 Mutex 节点: {}", lock_name);
                    for lock in group {
                        self.locks_counter.insert(lock.clone(), lock_node);
                    }
                }
                _ => {
                    let lock_name = format!("RwLock_{}", group_id);
                    let lock_p = Place::new(lock_name.clone(), 10, PlaceType::Lock);
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

    /// 简化 Petri 网中的状态,通过合并简单路径来减少网络的复杂度
    ///
    /// 具体步骤:
    /// 1. 找到所有入度和出度都≤1的节点作为起始点
    /// 2. 从每个起始点开始,向两个方向(前向和后向)搜索,找到可以合并的路径
    /// 3. 对于每条找到的路径:
    ///    - 确保路径的起点和终点都是 Place 节点
    ///    - 如果路径长度>3,则创建一个新的 Transition 节点来替代中间的节点
    ///    - 保持路径两端的 Place 节点不变,删除中间的所有节点
    /// 4. 最后统一删除所有被标记为需要移除的节点
    ///
    /// 这种简化可以显著减少 Petri 网的大小,同时保持其基本行为特性不变
    pub fn reduce_state(&mut self) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut all_nodes_to_remove = Vec::new();
        // 找到所有入度和出度都≤1的点
        for node in self.net.node_indices() {
            let in_degree = self.net.edges_directed(node, Direction::Incoming).count();
            let out_degree = self.net.edges_directed(node, Direction::Outgoing).count();

            if in_degree <= 1 && out_degree <= 1 {
                queue.push_back(node);
            }
        }
        // TODO: 设置新的截止条件，以防止 unsafe 操作被 merge
        while let Some(start) = queue.pop_front() {
            if visited.contains(&start) {
                continue;
            }

            // 从start开始BFS，找到一条链
            let mut chain = vec![start];
            let mut current = start;
            visited.insert(start);

            // 向两个方向遍历
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

            // 调整链，确保起始和结束都是Place
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
            // 检查调整后的链长度是否满足简化条件
            if chain.len() > 3 {
                // 确保chain不为空
                if chain.is_empty() {
                    continue;
                }
                let p1 = chain[0];
                let p2 = chain[chain.len() - 1];

                // 确保p1和p2都是Place
                if let (PetriNetNode::P(_), PetriNetNode::P(_)) = (&self.net[p1], &self.net[p2]) {
                    // 创建新的Transition
                    let new_trans = Transition::new(
                        format!("merged_trans_{}_{}", p1.index(), p2.index()),
                        ControlType::Goto,
                    );
                    let new_trans_idx = self.net.add_node(PetriNetNode::T(new_trans));

                    // 添加新边
                    self.net
                        .add_edge(p1, new_trans_idx, PetriNetEdge { label: 1u8 });
                    self.net
                        .add_edge(new_trans_idx, p2, PetriNetEdge { label: 1u8 });

                    // 将路径上的节点信息合并成一行输出
                    let path_info = chain[1..chain.len()]
                        .iter()
                        .map(|&node| match &self.net[node] {
                            PetriNetNode::P(place) => format!("P({})", place.name),
                            PetriNetNode::T(transition) => format!("T({})", transition.name),
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    log::debug!("Path: {}", path_info);
                    // 收集要删除的节点
                    all_nodes_to_remove.extend(chain[1..chain.len() - 1].iter().cloned());
                }
            }
        }
        // 在循环结束后统一删除节点
        if !all_nodes_to_remove.is_empty() {
            // 按索引从大到小排序
            all_nodes_to_remove.sort_by(|a, b| b.index().cmp(&a.index()));
            // 删除节点
            for node in all_nodes_to_remove {
                self.net.remove_node(node);
            }
        }
    }

    /// 分析并简化从起始节点到终止节点的路径，保留与特殊节点相连的路径
    /// 1. 使用DFS收集所有从start_node到end_node的路径
    /// 2. 标记与特殊节点相连的路径为有效路径
    /// 3. 收集需要保留的节点（出现在有效路径中的节点）
    /// 4. 删除仅出现在无效路径中的节点
    pub fn reduce_state_from(
        &mut self,
        start_node: NodeIndex,
        end_node: NodeIndex,
        special_nodes: &[NodeIndex],
    ) {
        // 存储所有从start到end的路径
        let mut all_paths: Vec<Vec<NodeIndex>> = Vec::new();
        // 存储有效路径（与特殊节点相连的路径）
        let mut valid_paths: HashSet<Vec<NodeIndex>> = HashSet::new();
        // 存储当前正在探索的路径
        let mut current_path: Vec<NodeIndex> = vec![start_node];
        // 记录已访问节点，避免简单环路
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        // DFS收集所有路径
        self.collect_paths(
            start_node,
            end_node,
            &mut all_paths,
            &mut current_path,
            &mut visited,
            special_nodes,
            &mut valid_paths,
        );

        // 收集所有需要保留的节点（出现在有效路径中的节点）
        let mut nodes_to_keep: HashSet<NodeIndex> = HashSet::new();
        for path in &valid_paths {
            nodes_to_keep.extend(path.iter().cloned());
        }

        // 收集所有可以删除的节点（出现在无效路径中且不在有效路径中的节点）
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

    /// 递归收集从起点到终点的所有路径
    ///
    /// 1. 如果到达终点，检查当前路径是否与特殊节点相连
    /// 2. 如果路径有效，添加到valid_paths中
    /// 3. 将当前路径添加到all_paths中
    /// 4. 递归探索所有未访问的邻居节点
    /// 5. 回溯时移除访问标记，允许节点在其他路径中被重复访问
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
            // 检查路径是否与特殊节点相连
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

        // 遍历 callgraph 收集信息
        for (instance_id, node) in self.callgraph.graph.node_references() {
            let instance = match node {
                CallGraphNode::WithBody(instance) => instance,
                _ => continue,
            };

            // 只分析本地函数
            if !instance.def_id().is_local() {
                continue;
            }

            let body = self.tcx.instance_mir(instance.def);
            let mut collector = BlockingCollector::new(instance_id, instance, body, self.tcx);
            collector.analyze();

            // 收集锁信息
            if !collector.lockguards.is_empty() {
                lockguards.insert(instance_id, collector.lockguards.clone());
                self.lock_info.extend(collector.lockguards);
            }

            // 收集条件变量信息
            if !collector.condvars.is_empty() {
                condvars.insert(instance_id, collector.condvars);
            }
        }

        // 处理条件变量
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

        // 返回收集到的锁信息供后续处理
        lockguards
    }

    pub fn get_current_mark(&self) -> HashSet<(NodeIndex, u8)> {
        let mut current_mark = HashSet::<(NodeIndex, u8)>::new();
        for node in self.net.node_indices() {
            match &self.net[node] {
                PetriNetNode::P(place) => {
                    if *place.tokens.read().unwrap() > 0 {
                        current_mark.insert((node.clone(), *place.tokens.read().unwrap() as u8));
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
                            let tokens = *place.tokens.read().unwrap();
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

    /// 验证Petri网的结构正确性
    ///
    /// 检查以下规则:
    /// 1. Transition节点的所有前驱和后继必须是Place节点
    /// 2. Place节点的所有前驱和后继必须是Transition节点
    /// 3. Place节点可以没有前驱或后继
    ///
    /// 返回:
    /// - Ok(()) 如果网络结构正确
    /// - Err(String) 包含错误描述的字符串
    pub fn verify_structure(&self) -> Result<Vec<NodeIndex>> {
        let mut transitions_to_remove = Vec::new();

        // 检查结构并收集需要删除的变迁
        for node_idx in self.net.node_indices() {
            match &self.net[node_idx] {
                PetriNetNode::T(transition) => {
                    // 检查变迁的后继
                    let successors: Vec<_> = self
                        .net
                        .neighbors_directed(node_idx, Direction::Outgoing)
                        .collect();

                    if successors.is_empty() {
                        transitions_to_remove.push(node_idx);
                        log::warn!("Found transition with no successors: {}", transition.name);
                        continue;
                    }

                    // 检查变迁的前驱和后继是否都是库所
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
                    // 检查库所的前驱和后继是否都是变迁
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
        // 从后向前删除节点以保持索引有效
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
                        terminal_states.push((node_idx.index(), *place.tokens.read().unwrap()));
                    }
                }
            }
        }
        terminal_states
    }
}
