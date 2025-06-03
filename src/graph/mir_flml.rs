use petgraph::graph::{Graph, NodeIndex};
use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;
use std::collections::{HashMap, HashSet};

/// 分析配置 - 决定中间表示包含哪些元素
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// 是否包含循环结构
    pub include_loops: bool,
    /// 是否包含异步操作
    pub include_async: bool,
    /// 是否包含不安全操作
    pub include_unsafe: bool,
    /// 是否包含数据依赖
    pub include_data_deps: bool,
    /// 是否包含线程同步
    pub include_thread_sync: bool,
    /// 是否包含函数调用细节
    pub include_call_details: bool,
    /// 是否简化控制流（合并简单的顺序块）
    pub simplify_control_flow: bool,
}

impl AnalysisConfig {
    /// 死锁检测配置
    pub fn deadlock_detection() -> Self {
        Self {
            include_loops: false,        // 循环不影响死锁检测
            include_async: false,        // 异步操作简化处理
            include_unsafe: false,       // 不安全操作不影响死锁
            include_data_deps: false,    // 数据依赖不影响死锁
            include_thread_sync: true,   // 线程同步是关键
            include_call_details: false, // 简化函数调用
            simplify_control_flow: true, // 简化控制流
        }
    }

    /// 数据竞争检测配置
    pub fn data_race_detection() -> Self {
        Self {
            include_loops: true,          // 循环中的数据访问很重要
            include_async: true,          // 异步操作可能导致数据竞争
            include_unsafe: true,         // 不安全操作是重点
            include_data_deps: true,      // 数据依赖是核心
            include_thread_sync: true,    // 同步操作影响数据访问
            include_call_details: true,   // 需要详细的调用信息
            simplify_control_flow: false, // 保留完整控制流
        }
    }

    /// 内存安全检测配置
    pub fn memory_safety() -> Self {
        Self {
            include_loops: true,
            include_async: false,
            include_unsafe: true, // 重点关注不安全操作
            include_data_deps: true,
            include_thread_sync: false,
            include_call_details: true,
            simplify_control_flow: false,
        }
    }
}

/// 抽象的中间表示节点
#[derive(Debug, Clone)]
pub enum AbstractNode {
    /// 程序入口点
    Entry { id: String, metadata: NodeMetadata },
    /// 程序出口点
    Exit { id: String, metadata: NodeMetadata },
    /// 同步点（锁、条件变量等）
    SyncPoint {
        sync_id: String,
        sync_type: SyncType,
        metadata: NodeMetadata,
    },
    /// 计算节点（抽象的计算单元）
    Computation {
        comp_id: String,
        comp_type: ComputationType,
        metadata: NodeMetadata,
    },
    /// 决策点（分支、选择）
    Decision {
        decision_id: String,
        branches: Vec<String>,
        metadata: NodeMetadata,
    },
    /// 资源节点（内存、文件等）
    Resource {
        resource_id: String,
        resource_type: ResourceType,
        metadata: NodeMetadata,
    },
}

/// 节点元数据
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    /// 源码位置
    pub span: String,
    /// 相关的DefId（如果有）
    pub def_id: Option<DefId>,
    /// 基本块ID（如果有）
    pub bb_id: Option<usize>,
    /// 自定义属性
    pub attributes: HashMap<String, String>,
}

/// 同步类型
#[derive(Debug, Clone)]
pub enum SyncType {
    /// 互斥锁获取
    MutexAcquire,
    /// 互斥锁释放
    MutexRelease,
    /// 读写锁读取
    RwLockRead,
    /// 读写锁写入
    RwLockWrite,
    /// 读写锁释放
    RwLockRelease,
    /// 条件变量等待
    CondVarWait,
    /// 条件变量通知
    CondVarNotify,
    /// 原子操作
    AtomicOp(String),
    /// 线程创建
    ThreadSpawn,
    /// 线程等待
    ThreadJoin,
    /// 通道发送
    ChannelSend,
    /// 通道接收
    ChannelRecv,
}

/// 计算类型
#[derive(Debug, Clone)]
pub enum ComputationType {
    /// 普通计算
    Normal,
    /// 函数调用
    FunctionCall(DefId),
    /// 异步操作
    AsyncOp(AsyncOpType),
    /// 不安全操作
    UnsafeOp(UnsafeOpType),
    /// 循环体
    LoopBody,
}

/// 资源类型
#[derive(Debug, Clone)]
pub enum ResourceType {
    /// 内存位置
    Memory(String),
    /// 文件句柄
    File(String),
    /// 网络连接
    Network(String),
    /// 自定义资源
    Custom(String),
}

/// 异步操作类型
#[derive(Debug, Clone)]
pub enum AsyncOpType {
    AsyncCall(DefId),
    Await,
    AsyncBlock,
    AsyncGen,
}

/// 不安全操作类型
#[derive(Debug, Clone)]
pub enum UnsafeOpType {
    UnsafeCall(DefId),
    RawPtrDeref,
    MemoryOp,
    FFICall,
}

/// 抽象的边类型
#[derive(Debug, Clone)]
pub enum AbstractEdge {
    /// 控制流边
    ControlFlow { condition: Option<String> },
    /// 同步边（表示同步依赖）
    Synchronization {
        sync_type: SyncType,
        resource_id: String,
    },
    /// 数据流边
    DataFlow {
        var_id: String,
        access_type: AccessType,
    },
    /// 调用边
    Call { def_id: DefId, call_type: CallType },
    /// 依赖边（抽象的依赖关系）
    Dependency {
        dep_type: DependencyType,
        strength: DependencyStrength,
    },
}

/// 访问类型
#[derive(Debug, Clone)]
pub enum AccessType {
    Read,
    Write,
    ReadWrite,
}

/// 调用类型
#[derive(Debug, Clone)]
pub enum CallType {
    Normal,
    Closure,
    Virtual,
    Inline,
    Async,
}

/// 依赖类型
#[derive(Debug, Clone)]
pub enum DependencyType {
    /// 数据依赖
    Data,
    /// 控制依赖
    Control,
    /// 同步依赖
    Sync,
    /// 时序依赖
    Temporal,
}

/// 依赖强度
#[derive(Debug, Clone)]
pub enum DependencyStrength {
    /// 强依赖（必须满足）
    Strong,
    /// 弱依赖（可能满足）
    Weak,
    /// 条件依赖（在某些条件下满足）
    Conditional(String),
}

/// 抽象中间表示图
pub struct AbstractIR<'tcx> {
    /// 图结构
    pub graph: Graph<AbstractNode, AbstractEdge>,
    /// 类型上下文
    pub tcx: TyCtxt<'tcx>,
    /// 分析配置
    pub config: AnalysisConfig,
    /// 节点索引映射
    pub node_map: HashMap<String, NodeIndex>,
    /// 同步点映射
    pub sync_points: HashMap<String, NodeIndex>,
    /// 资源映射
    pub resources: HashMap<String, NodeIndex>,
    /// 活跃的同步操作
    pub active_syncs: HashSet<String>,
}

impl<'tcx> AbstractIR<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, config: AnalysisConfig) -> Self {
        Self {
            graph: Graph::new(),
            tcx,
            config,
            node_map: HashMap::new(),
            sync_points: HashMap::new(),
            resources: HashMap::new(),
            active_syncs: HashSet::new(),
        }
    }

    /// 添加节点（根据配置决定是否实际添加）
    pub fn add_node(&mut self, node: AbstractNode) -> Option<NodeIndex> {
        let should_add = match &node {
            AbstractNode::Computation { comp_type, .. } => match comp_type {
                ComputationType::LoopBody => self.config.include_loops,
                ComputationType::AsyncOp(_) => self.config.include_async,
                ComputationType::UnsafeOp(_) => self.config.include_unsafe,
                _ => true,
            },
            AbstractNode::SyncPoint { .. } => self.config.include_thread_sync,
            _ => true,
        };

        if should_add {
            let node_id = self.get_node_id(&node);
            let index = self.graph.add_node(node);
            self.node_map.insert(node_id, index);
            Some(index)
        } else {
            None
        }
    }

    /// 添加边（根据配置决定是否实际添加）
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge: AbstractEdge) -> bool {
        let should_add = match &edge {
            AbstractEdge::DataFlow { .. } => self.config.include_data_deps,
            AbstractEdge::Synchronization { .. } => self.config.include_thread_sync,
            AbstractEdge::Call { call_type, .. } => match call_type {
                CallType::Async => self.config.include_async,
                _ => self.config.include_call_details,
            },
            AbstractEdge::Dependency { dep_type, .. } => match dep_type {
                DependencyType::Data => self.config.include_data_deps,
                DependencyType::Sync => self.config.include_thread_sync,
                _ => true,
            },
            _ => true,
        };

        if should_add {
            self.graph.add_edge(from, to, edge);
            true
        } else {
            false
        }
    }

    /// 获取节点ID
    fn get_node_id(&self, node: &AbstractNode) -> String {
        match node {
            AbstractNode::Entry { id, .. } => id.clone(),
            AbstractNode::Exit { id, .. } => id.clone(),
            AbstractNode::SyncPoint { sync_id, .. } => sync_id.clone(),
            AbstractNode::Computation { comp_id, .. } => comp_id.clone(),
            AbstractNode::Decision { decision_id, .. } => decision_id.clone(),
            AbstractNode::Resource { resource_id, .. } => resource_id.clone(),
        }
    }

    /// 简化图结构（如果配置允许）
    pub fn simplify(&mut self) {
        if self.config.simplify_control_flow {
            self.merge_sequential_nodes();
        }
    }

    /// 合并顺序节点
    fn merge_sequential_nodes(&mut self) {
        // 实现节点合并逻辑
        // 这里可以合并只有单一前驱和后继的计算节点
    }

    /// 获取所有同步点
    pub fn get_sync_points(&self) -> Vec<NodeIndex> {
        self.sync_points.values().cloned().collect()
    }

    /// 获取所有资源节点
    pub fn get_resources(&self) -> Vec<NodeIndex> {
        self.resources.values().cloned().collect()
    }
}

/// MIR到FLML的转换器
pub struct MirToFLMLConverter<'tcx> {
    tcx: TyCtxt<'tcx>,
    flml_ir: AbstractIR<'tcx>,
}

impl<'tcx> MirToFLMLConverter<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, config: AnalysisConfig) -> Self {
        Self {
            tcx,
            flml_ir: AbstractIR::new(tcx, config),
        }
    }

    /// 转换函数体
    pub fn convert_function(&mut self, def_id: DefId, body: &rustc_middle::mir::Body<'tcx>) {
        let func_name = self.tcx.def_path_str(def_id);

        // 创建函数入口和出口节点
        let entry_node = self.flml_ir.add_node(AbstractNode::Entry {
            id: format!("{}_entry", func_name),
            metadata: NodeMetadata {
                span: format!("{:?}", body.span),
                def_id: Some(def_id),
                bb_id: None,
                attributes: HashMap::new(),
            },
        });

        let exit_node = self.flml_ir.add_node(AbstractNode::Exit {
            id: format!("{}_exit", func_name),
            metadata: NodeMetadata {
                span: format!("{:?}", body.span),
                def_id: Some(def_id),
                bb_id: None,
                attributes: HashMap::new(),
            },
        });

        // 转换基本块
        let mut bb_nodes = HashMap::new();
        for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
            if bb_data.is_cleanup {
                continue;
            }

            let bb_node = self.convert_basic_block(bb_idx.index(), bb_data, &func_name, body);
            if let Some(node) = bb_node {
                bb_nodes.insert(bb_idx.index(), node);
            }
        }

        // 连接入口到第一个基本块
        if let Some(&first_bb) = bb_nodes.get(&0) {
            if let (Some(entry), Some(_)) = (entry_node, exit_node) {
                self.flml_ir.add_edge(
                    entry,
                    first_bb,
                    AbstractEdge::ControlFlow { condition: None },
                );
            }
        }

        // 处理基本块之间的控制流
        for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
            if bb_data.is_cleanup {
                continue;
            }

            if let Some(&current_bb) = bb_nodes.get(&bb_idx.index()) {
                self.connect_basic_block_edges(current_bb, bb_data, &bb_nodes, exit_node);
            }
        }
    }

    /// 转换基本块
    fn convert_basic_block(
        &mut self,
        bb_idx: usize,
        bb_data: &rustc_middle::mir::BasicBlockData<'tcx>,
        func_name: &str,
        body: &rustc_middle::mir::Body<'tcx>,
    ) -> Option<NodeIndex> {
        let span = bb_data
            .terminator
            .as_ref()
            .map(|term| format!("{:?}", term.source_info.span))
            .unwrap_or_else(|| "unknown".to_string());

        // 分析基本块中的语句
        let mut comp_type = ComputationType::Normal;
        let mut has_sync_ops = false;

        // 检查语句中的特殊操作
        for stmt in &bb_data.statements {
            if let rustc_middle::mir::StatementKind::Assign(box (_, rvalue)) = &stmt.kind {
                match rvalue {
                    rustc_middle::mir::Rvalue::Use(_) => {
                        // 普通使用
                    }
                    _ => {
                        // 其他复杂操作
                    }
                }
            }
        }

        // 检查终止符
        if let Some(terminator) = &bb_data.terminator {
            match &terminator.kind {
                rustc_middle::mir::TerminatorKind::Call { func, .. } => {
                    let func_ty = func.ty(body, self.tcx);
                    if let rustc_middle::ty::TyKind::FnDef(callee_def_id, _) = func_ty.kind() {
                        let callee_name = self.tcx.def_path_str(*callee_def_id);

                        // 检测同步操作
                        if callee_name.contains("lock") || callee_name.contains("mutex") {
                            has_sync_ops = true;
                        } else if callee_name.contains("spawn") || callee_name.contains("join") {
                            has_sync_ops = true;
                        } else if callee_name.contains("async") || callee_name.contains("await") {
                            comp_type =
                                ComputationType::AsyncOp(AsyncOpType::AsyncCall(*callee_def_id));
                        } else if callee_name.contains("unsafe") {
                            comp_type =
                                ComputationType::UnsafeOp(UnsafeOpType::UnsafeCall(*callee_def_id));
                        } else {
                            comp_type = ComputationType::FunctionCall(*callee_def_id);
                        }
                    }
                }
                rustc_middle::mir::TerminatorKind::SwitchInt { .. } => {
                    // 这是一个决策点
                    return self.flml_ir.add_node(AbstractNode::Decision {
                        decision_id: format!("{}_bb{}_decision", func_name, bb_idx),
                        branches: vec!["true".to_string(), "false".to_string()],
                        metadata: NodeMetadata {
                            span,
                            def_id: None,
                            bb_id: Some(bb_idx),
                            attributes: HashMap::new(),
                        },
                    });
                }
                _ => {}
            }
        }

        // 如果有同步操作，创建同步点
        if has_sync_ops {
            return self.flml_ir.add_node(AbstractNode::SyncPoint {
                sync_id: format!("{}_bb{}_sync", func_name, bb_idx),
                sync_type: SyncType::MutexAcquire, // 简化处理
                metadata: NodeMetadata {
                    span,
                    def_id: None,
                    bb_id: Some(bb_idx),
                    attributes: HashMap::new(),
                },
            });
        }

        // 创建计算节点
        self.flml_ir.add_node(AbstractNode::Computation {
            comp_id: format!("{}_bb{}_comp", func_name, bb_idx),
            comp_type,
            metadata: NodeMetadata {
                span,
                def_id: None,
                bb_id: Some(bb_idx),
                attributes: HashMap::new(),
            },
        })
    }

    /// 连接基本块之间的边
    fn connect_basic_block_edges(
        &mut self,
        current_bb: NodeIndex,
        bb_data: &rustc_middle::mir::BasicBlockData<'tcx>,
        bb_nodes: &HashMap<usize, NodeIndex>,
        exit_node: Option<NodeIndex>,
    ) {
        if let Some(terminator) = &bb_data.terminator {
            match &terminator.kind {
                rustc_middle::mir::TerminatorKind::Goto { target } => {
                    if let Some(&target_node) = bb_nodes.get(&target.index()) {
                        self.flml_ir.add_edge(
                            current_bb,
                            target_node,
                            AbstractEdge::ControlFlow { condition: None },
                        );
                    }
                }
                rustc_middle::mir::TerminatorKind::SwitchInt { targets, .. } => {
                    for target in targets.all_targets() {
                        if let Some(&target_node) = bb_nodes.get(&target.index()) {
                            self.flml_ir.add_edge(
                                current_bb,
                                target_node,
                                AbstractEdge::ControlFlow {
                                    condition: Some(format!("branch_{}", target.index())),
                                },
                            );
                        }
                    }
                }
                rustc_middle::mir::TerminatorKind::Return => {
                    if let Some(exit) = exit_node {
                        self.flml_ir.add_edge(
                            current_bb,
                            exit,
                            AbstractEdge::ControlFlow { condition: None },
                        );
                    }
                }
                rustc_middle::mir::TerminatorKind::Call {
                    target: Some(target),
                    ..
                } => {
                    if let Some(&target_node) = bb_nodes.get(&target.index()) {
                        self.flml_ir.add_edge(
                            current_bb,
                            target_node,
                            AbstractEdge::ControlFlow { condition: None },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// 获取生成的FLML IR
    pub fn get_flml_ir(self) -> AbstractIR<'tcx> {
        self.flml_ir
    }

    /// 导出为JSON格式
    pub fn export_to_json(&self) -> Result<String, serde_json::Error> {
        use serde_json::json;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        for node_idx in self.flml_ir.graph.node_indices() {
            if let Some(node) = self.flml_ir.graph.node_weight(node_idx) {
                nodes.push(json!({
                    "id": node_idx.index(),
                    "type": format!("{:?}", node),
                    "metadata": match node {
                        AbstractNode::Entry { metadata, .. } |
                        AbstractNode::Exit { metadata, .. } |
                        AbstractNode::SyncPoint { metadata, .. } |
                        AbstractNode::Computation { metadata, .. } |
                        AbstractNode::Decision { metadata, .. } |
                        AbstractNode::Resource { metadata, .. } => {
                            json!({
                                "span": metadata.span,
                                "def_id": metadata.def_id.map(|id| format!("{:?}", id)),
                                "bb_id": metadata.bb_id,
                                "attributes": metadata.attributes
                            })
                        }
                    }
                }));
            }
        }

        for edge_idx in self.flml_ir.graph.edge_indices() {
            if let Some(edge) = self.flml_ir.graph.edge_weight(edge_idx) {
                let (source, target) = self.flml_ir.graph.edge_endpoints(edge_idx).unwrap();
                edges.push(json!({
                    "source": source.index(),
                    "target": target.index(),
                    "type": format!("{:?}", edge)
                }));
            }
        }

        serde_json::to_string_pretty(&json!({
            "nodes": nodes,
            "edges": edges,
            "config": format!("{:?}", self.flml_ir.config)
        }))
    }
}
