use crate::analysis::pointsto::AliasId;
use crate::graph::callgraph::{CallGraph, CallGraphNode, InstanceId};
use crate::utils::format_name;
use petgraph::csr::IndexType;
use petgraph::visit::{IntoNodeReferences, NodeRef};
use rustc_hash::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{
    Body, Local, LocalKind, Location, Operand, Place, ProjectionElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use rustc_middle::ty::{Instance, TyCtxt, TyKind};
use serde::Serialize;

/// 表示一个 unsafe 块的信息
#[derive(Debug, Clone, Serialize)]
pub struct UnsafeBlockInfo {
    /// unsafe 块的位置
    pub span: String,
    /// unsafe 块所在的基本块
    pub block: usize,
    /// unsafe 块中包含的语句位置
    pub locations: Vec<String>,
}

/// 表示对 unsafe 变量的操作类型
#[derive(Debug, Clone, Serialize)]
pub enum UnsafeOperation {
    Read(String),   // 读取操作,包含位置信息
    Write(String),  // 写入操作
    Deref(String),  // 解引用
    AddrOf(String), // 取地址
    Cast(String),   // 类型转换
    Call(String),   // 作为函数参数
}

/// 表示一个 unsafe 的局部变量信息
#[derive(Debug, Clone, Serialize)]
pub struct UnsafePlaceInfo {
    /// 局部变量
    pub local: usize,
    /// 变量的类型字符串
    pub ty_string: String,
    /// 变量定义的位置
    pub span: String,
    /// 变量是否来自 unsafe 函数的参数
    pub is_param: bool,
    /// 记录所有对该变量的操作
    pub operations: Vec<UnsafeOperation>,
}

/// 收集函数中所有的 unsafe 信息
#[derive(Debug, Clone, Default, Serialize)]
pub struct UnsafeInfo {
    /// 函数是否被标记为 unsafe
    pub is_unsafe_fn: bool,
    /// 函数中的 unsafe 块
    pub unsafe_blocks: Vec<UnsafeBlockInfo>,
    /// 函数中的 unsafe 局部变量
    pub unsafe_places: FxHashMap<usize, UnsafePlaceInfo>,
}

pub struct UnsafeData {
    pub unsafe_places: FxHashMap<AliasId, String>,
}

impl Default for UnsafeData {
    fn default() -> Self {
        Self {
            unsafe_places: FxHashMap::default(),
        }
    }
}

impl UnsafeData {
    pub fn get_or_insert(&mut self, alias_id: AliasId, span: String) {
        for (id, old_span) in self.unsafe_places.iter() {
            if *id == alias_id && *old_span == span {
                return;
            }
        }
        self.unsafe_places.insert(alias_id, span);
    }
}

pub struct UnsafeDataInfo {
    /// 局部变量
    pub local: Local,
    /// 变量定义的位置
    pub span: String,
}

impl UnsafeInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 收集函数中的 unsafe 信息
    pub fn collect<'tcx>(
        instance: Instance<'tcx>,
        instance_id: Option<InstanceId>,
        body: &Body<'tcx>,
        tcx: TyCtxt<'tcx>,
    ) -> (UnsafeData, UnsafeInfo) {
        let mut collector = UnsafeCollector::new(instance, instance_id, body, tcx);
        let unsafe_data = collector.analyze();
        (unsafe_data, collector.finish())
    }
}

struct UnsafeCollector<'a, 'tcx> {
    instance: Instance<'tcx>,
    instance_id: Option<InstanceId>,
    body: &'a Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    info: UnsafeInfo,
}

impl<'a, 'tcx> UnsafeCollector<'a, 'tcx> {
    fn new(
        instance: Instance<'tcx>,
        instance_id: Option<InstanceId>,
        body: &'a Body<'tcx>,
        tcx: TyCtxt<'tcx>,
    ) -> Self {
        Self {
            instance,
            instance_id,
            body,
            tcx,
            info: UnsafeInfo::new(),
        }
    }

    fn analyze(&mut self) -> UnsafeData {
        let mut unsafe_data = UnsafeData::default();
        // 首先收集所有 unsafe 变量
        self.collect_unsafe_locals(&mut unsafe_data);

        // 然后再分析语句和操作
        self.info.is_unsafe_fn = self.check_unsafe_fn();
        self.visit_body(self.body);
        unsafe_data
    }

    fn collect_unsafe_locals(&mut self, unsafe_data: &mut UnsafeData) {
        // 预先收集所有局部变量
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            let ty = local_decl.ty;
            if ty.is_unsafe_ptr() {
                let info = UnsafePlaceInfo {
                    local: local.index(),
                    ty_string: ty.to_string(),
                    span: format!("{:?}", local_decl.source_info.span),
                    is_param: matches!(self.body.local_kind(local), LocalKind::Arg),
                    operations: Vec::new(),
                };
                let unsafe_data_info = UnsafeDataInfo {
                    local: local,
                    span: format!("{:?}", local_decl.source_info.span),
                };
                self.info.unsafe_places.insert(local.index(), info);
                match self.instance_id {
                    Some(instance_id) => {
                        unsafe_data
                            .get_or_insert(AliasId::new(instance_id, local), unsafe_data_info.span);
                    }
                    None => {}
                }
            }
        }
    }

    pub fn finish(self) -> UnsafeInfo {
        self.info
    }

    fn check_unsafe_fn(&self) -> bool {
        let def_id = self.instance.def_id();
        let hir_id = self.tcx.local_def_id_to_hir_id(def_id.expect_local());

        // Closure 的 safety 总是返回 None
        if let Some(fn_sig) = self.tcx.hir().fn_sig_by_hir_id(hir_id) {
            matches!(fn_sig.header.safety, rustc_hir::Safety::Unsafe)
        } else {
            false
        }
    }

    fn is_unsafe_operation(&mut self, statement: &Statement<'tcx>, location: Location) -> bool {
        match &statement.kind {
            StatementKind::Assign(box (place, rvalue)) => {
                let loc_str = format!("{:?}", location);

                // 记录写操作
                if self.info.unsafe_places.contains_key(&place.local.index()) {
                    if let Some(place_info) = self.info.unsafe_places.get_mut(&place.local.index())
                    {
                        place_info
                            .operations
                            .push(UnsafeOperation::Write(loc_str.clone()));
                    }
                }

                // 记录读操作和其他操作
                match rvalue {
                    Rvalue::Use(operand) => {
                        self.check_operand_usage(operand, &loc_str);
                        self.is_unsafe_operand(operand)
                    }
                    Rvalue::Ref(_, _, place) => {
                        if self.info.unsafe_places.contains_key(&place.local.index()) {
                            if let Some(place_info) =
                                self.info.unsafe_places.get_mut(&place.local.index())
                            {
                                place_info.operations.push(UnsafeOperation::AddrOf(loc_str));
                            }
                            true
                        } else {
                            false
                        }
                    }
                    Rvalue::Cast(_, operand, _) => {
                        if self.is_unsafe_operand(operand) {
                            if let Operand::Copy(place) | Operand::Move(place) = operand {
                                if let Some(place_info) =
                                    self.info.unsafe_places.get_mut(&place.local.index())
                                {
                                    place_info.operations.push(UnsafeOperation::Cast(loc_str));
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn check_operand_usage(&mut self, operand: &Operand<'tcx>, loc_str: &str) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if self.info.unsafe_places.contains_key(&place.local.index()) {
                    if let Some(place_info) = self.info.unsafe_places.get_mut(&place.local.index())
                    {
                        place_info
                            .operations
                            .push(UnsafeOperation::Read(loc_str.to_string()));

                        // 检查解引用
                        if place
                            .projection
                            .iter()
                            .any(|elem| matches!(elem, ProjectionElem::Deref))
                        {
                            place_info
                                .operations
                                .push(UnsafeOperation::Deref(loc_str.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn is_unsafe_place(&self, place: Place<'tcx>) -> bool {
        // 检查是否涉及原始指针解引用
        place
            .projection
            .iter()
            .any(|elem| matches!(elem, ProjectionElem::Deref))
            && self.info.unsafe_places.contains_key(&place.local.index())
    }

    #[allow(dead_code)]
    fn is_unsafe_rvalue(&self, rvalue: &Rvalue<'tcx>) -> bool {
        match rvalue {
            Rvalue::Ref(_, _, place) => self.is_unsafe_place(*place),
            Rvalue::Cast(_, operand, _) => self.is_unsafe_operand(operand),
            _ => false,
        }
    }

    fn is_unsafe_operand(&self, operand: &Operand<'tcx>) -> bool {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.is_unsafe_place(*place),
            _ => false,
        }
    }
}

impl<'a, 'tcx> Visitor<'tcx> for UnsafeCollector<'a, 'tcx> {
    // 访问语句
    fn visit_statement(&mut self, statement: &Statement<'tcx>, location: Location) {
        if self.is_unsafe_operation(statement, location) {
            // 如果当前语句在 unsafe 块中
            if let Some(block_info) = self
                .info
                .unsafe_blocks
                .iter_mut()
                .find(|block| block.block.index() == location.block.index())
            {
                block_info.locations.push(format!("{:?}", location));
            } else {
                let unsafe_block = UnsafeBlockInfo {
                    span: format!("{:?}", statement.source_info.span),
                    block: location.block.index(),
                    locations: vec![format!("{:?}", location)],
                };
                self.info.unsafe_blocks.push(unsafe_block);
            }
        }

        self.super_statement(statement, location);
    }

    // 访问终止符
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        // 检查终止符是否在unsafe上下文中
        match terminator.kind {
            // 特别关注函数调用
            TerminatorKind::Call { ref func, .. } => {
                let func_ty = func.ty(self.body, self.tcx);
                if let TyKind::FnDef(def_id, _) = func_ty.kind() {
                    // 如果调用的是unsafe函数
                    if self.tcx.is_mir_available(*def_id) {
                        if def_id.is_local() {
                            let hir_id = self.tcx.local_def_id_to_hir_id(def_id.expect_local());
                            if matches!(
                                self.tcx
                                    .hir()
                                    .fn_sig_by_hir_id(hir_id)
                                    .unwrap()
                                    .header
                                    .safety,
                                rustc_hir::Safety::Unsafe
                            ) {
                                self.info.unsafe_blocks.push(UnsafeBlockInfo {
                                    span: format!("{:?}", terminator.source_info.span),
                                    block: location.block.index(),
                                    locations: vec![format!("{:?}", location)],
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // let loc_str = format!("{:?}", location);
        // // 检查函数参数中的 unsafe 变量
        // for arg in args {
        //     if let Operand::Copy(place) | Operand::Move(place) = arg {
        //         if let Some(place_info) = self.info.unsafe_places.get_mut(&place.local.index()) {
        //             place_info
        //                 .operations
        //                 .push(UnsafeOperation::Call(loc_str.clone()));
        //         }
        //     }
        // }

        self.super_terminator(terminator, location);
    }
}

/// 用于分析和收集整个crate中的unsafe使用情况
pub struct UnsafeAnalyzer<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    callgraph: &'a CallGraph<'tcx>,
    crate_name: String,
}

impl<'a, 'tcx> UnsafeAnalyzer<'a, 'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, callgraph: &'a CallGraph<'tcx>, crate_name: String) -> Self {
        Self {
            tcx,
            callgraph,
            crate_name,
        }
    }

    /// 分析当前crate中的所有unsafe使用
    pub fn analyze(&self) -> (FxHashMap<DefId, UnsafeInfo>, UnsafeData) {
        let mut unsafe_info = FxHashMap::default();
        let mut unsafe_data = UnsafeData::default();
        for node_ref in self.callgraph.graph.node_references() {
            if let CallGraphNode::WithBody(instance) = node_ref.weight() {
                let def_id = instance.def_id();
                if def_id.is_local()
                    && format_name(def_id).starts_with(&self.crate_name)
                    && self.tcx.is_mir_available(def_id)
                {
                    let body = self.tcx.optimized_mir(def_id);
                    let instance_id = self.callgraph.instance_to_index(instance);
                    let unsafe_collector =
                        UnsafeInfo::collect(*instance, instance_id, body, self.tcx);
                    unsafe_info.insert(def_id, unsafe_collector.1);
                    unsafe_data
                        .unsafe_places
                        .extend(unsafe_collector.0.unsafe_places);
                }
            }
        }

        (unsafe_info, unsafe_data)
    }
}
