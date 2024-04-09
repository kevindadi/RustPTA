use rustc_data_structures::fx::FxHashMap;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::BasicBlock;
use rustc_middle::mir::Body;
use rustc_middle::mir::Local;
use rustc_middle::mir::Operand;
use rustc_middle::mir::Place;
use rustc_middle::mir::ProjectionElem;
use rustc_middle::mir::Rvalue;
use rustc_middle::mir::SourceInfo;
use rustc_middle::mir::StatementKind;
use rustc_middle::mir::Terminator;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::mir::UnwindAction;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::Span;
use std::cmp::min;
use std::vec::Vec;

#[derive(Debug, Clone)]
pub struct Node {
    pub index: usize,
    pub local: usize,
    need_drop: bool,
    so_so: bool,
    pub kind: usize,
    pub father: usize,
    pub alias: Vec<usize>,
    pub alive: isize,
    pub sons: FxHashMap<usize, usize>,
    pub field_info: Vec<usize>,
}

impl Node {
    pub fn new(index: usize, local: usize, need_drop: bool, so_so: bool) -> Node {
        let mut eq = Vec::new();
        eq.push(local);
        Node {
            index: index,
            local: local,
            need_drop: need_drop,
            father: local,
            alias: eq,
            alive: 0,
            so_so: so_so,
            kind: 0,
            sons: FxHashMap::default(),
            field_info: Vec::<usize>::new(),
        }
    }

    pub fn need_drop(&self) -> bool {
        return self.need_drop;
    }

    pub fn so_so(&self) -> bool {
        return self.so_so;
    }

    pub fn dead(&mut self) {
        self.alive = -1;
    }

    pub fn is_alive(&self) -> bool {
        return self.alive > -1;
    }

    pub fn is_tuple(&self) -> bool {
        return self.kind == 2;
    }

    pub fn is_ptr(&self) -> bool {
        return self.kind == 1 || self.kind == 4;
    }

    pub fn is_ref(&self) -> bool {
        return self.kind == 4;
    }

    pub fn is_corner_case(&self) -> bool {
        return self.kind == 3;
    }
}

#[derive(Debug, Clone)]
pub struct ReturnAssign {
    pub left_index: usize,
    pub left: Vec<usize>,
    pub left_so_so: bool,
    pub left_need_drop: bool,
    pub right_index: usize,
    pub right: Vec<usize>,
    pub right_so_so: bool,
    pub right_need_drop: bool,
    pub atype: usize,
}

impl ReturnAssign {
    pub fn new(
        atype: usize,
        left_index: usize,
        left_so_so: bool,
        left_need_drop: bool,
        right_index: usize,
        right_so_so: bool,
        right_need_drop: bool,
    ) -> ReturnAssign {
        let left = Vec::<usize>::new();
        let right = Vec::<usize>::new();
        ReturnAssign {
            left_index: left_index,
            left: left,
            left_so_so: left_so_so,
            left_need_drop: left_need_drop,
            right_index: right_index,
            right: right,
            right_so_so: right_so_so,
            right_need_drop: right_need_drop,
            atype: atype,
        }
    }

    pub fn valuable(&self) -> bool {
        return self.left_so_so && self.right_so_so;
    }
}

#[derive(Clone)]
pub struct ReturnResults {
    pub arg_size: usize,
    pub assignments: Vec<ReturnAssign>,
    pub dead: FxHashSet<usize>,
}

impl ReturnResults {
    pub fn new(arg_size: usize) -> ReturnResults {
        let assignments = Vec::<ReturnAssign>::new();
        let dead = FxHashSet::default();
        ReturnResults {
            arg_size: arg_size,
            assignments: assignments,
            dead: dead,
        }
    }
}

//self-defined assignments structure.
#[derive(Debug, Clone)]
pub struct Assignment<'tcx> {
    pub left: Place<'tcx>,
    pub right: Place<'tcx>,
    pub atype: usize,
    pub span: Span,
}

impl<'tcx> Assignment<'tcx> {
    pub fn new(
        left: Place<'tcx>,
        right: Place<'tcx>,
        atype: usize,
        span: Span,
    ) -> Assignment<'tcx> {
        Assignment {
            left: left,
            right: right,
            atype: atype,
            span: span,
        }
    }
}

//self-defined basicblock structure.
#[derive(Debug, Clone)]
pub struct BlockNode<'tcx> {
    pub index: usize,
    pub is_cleanup: bool,
    pub next: FxHashSet<usize>,
    pub assignments: Vec<Assignment<'tcx>>,
    pub calls: Vec<Terminator<'tcx>>,
    pub drops: Vec<Terminator<'tcx>>,
    //store the index of the sub-blocks as the current node is the root node of a SCC.
    pub sub_blocks: Vec<usize>,
    //store the const value defined in this block;
    pub const_value: Vec<(usize, usize)>,
    //store switch stmts in current block for the path filtering in path-sensitive analysis.
    pub switch_stmts: Vec<Terminator<'tcx>>,
}

impl<'tcx> BlockNode<'tcx> {
    pub fn new(index: usize, is_cleanup: bool) -> BlockNode<'tcx> {
        BlockNode {
            index: index,
            is_cleanup: is_cleanup,
            next: FxHashSet::<usize>::default(),
            assignments: Vec::<Assignment<'tcx>>::new(),
            calls: Vec::<Terminator<'tcx>>::new(),
            drops: Vec::<Terminator<'tcx>>::new(),
            sub_blocks: Vec::<usize>::new(),
            const_value: Vec::<(usize, usize)>::new(),
            switch_stmts: Vec::<Terminator<'tcx>>::new(),
        }
    }

    pub fn push(&mut self, index: usize) {
        self.next.insert(index);
    }
}

pub struct SafeDropGraph<'tcx> {
    pub def_id: DefId,
    pub span: Span,
    // contains all varibles (including fields) as nodes.
    pub nodes: Vec<Node>,
    // contains all blocks in the CFG
    pub blocks: Vec<BlockNode<'tcx>>,
    pub arg_size: usize,
    // we shrink a SCC into a node and use a father node to represent the SCC.
    pub father_block: Vec<usize>,
    // record the constant value during safedrop checking.
    pub constant_bool: FxHashMap<usize, usize>,
    // used for tarjan algorithmn.
    pub count: usize,
    // contains the return results for inter-procedure analysis.
    pub return_results: ReturnResults,
    // used for filtering duplicate alias assignments in return results.
    pub return_set: FxHashSet<(usize, usize)>,
    // record the information of bugs for the function.
    pub bug_records: BugRecords,
    // a threhold to avoid path explosion.
    pub visit_times: usize,
}

impl<'tcx> SafeDropGraph<'tcx> {
    pub fn new(my_body: &Body<'tcx>, tcx: TyCtxt<'tcx>, def_id: DefId) -> SafeDropGraph<'tcx> {
        // handle variables
        let locals = &my_body.local_decls;
        let arg_size = my_body.arg_count;
        let mut nodes = Vec::<Node>::new();
        let param_env = tcx.param_env(def_id);
        for ld in 0..locals.len() {
            let temp = Local::from(ld);
            let need_drop = locals[temp].ty.needs_drop(tcx, param_env);
            let so_so = so_so(locals[temp].ty);
            let mut node = Node::new(ld, ld, need_drop, need_drop || !so_so);
            node.kind = kind(locals[temp].ty);
            nodes.push(node);
        }

        let ref basicblocks = my_body.basic_blocks;
        let mut blocks = Vec::<BlockNode<'tcx>>::new();
        let mut father_block = Vec::<usize>::new();

        // handle each basicblock
        for i in 0..basicblocks.len() {
            father_block.push(i);
            let iter = BasicBlock::from(i);
            let terminator = basicblocks[iter].terminator.clone().unwrap();
            let mut current_node = BlockNode::new(i, basicblocks[iter].is_cleanup);

            // handle general statements
            for statement in &basicblocks[iter].statements {
                if let StatementKind::Assign(ref assign) = statement.kind {
                    let left_ssa = assign.0.local.as_usize();
                    let left = assign.0.clone();
                    match assign.1 {
                        Rvalue::Use(ref x) => match x {
                            Operand::Copy(ref p) => {
                                let right_ssa = p.local.as_usize();
                                if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                    let right = p.clone();
                                    let assign = Assignment::new(
                                        left,
                                        right,
                                        0,
                                        statement.source_info.span.clone(),
                                    );
                                    current_node.assignments.push(assign);
                                }
                            }
                            Operand::Move(ref p) => {
                                let right_ssa = p.local.as_usize();
                                if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                    let right = p.clone();
                                    let assign = Assignment::new(
                                        left,
                                        right,
                                        1,
                                        statement.source_info.span.clone(),
                                    );
                                    current_node.assignments.push(assign);
                                }
                            }
                            Operand::Constant(ref constant) => {
                                if let None = constant.literal.try_to_scalar() {
                                    continue;
                                }
                                if let Err(_tmp) = constant
                                    .literal
                                    .try_to_scalar()
                                    .clone()
                                    .unwrap()
                                    .try_to_int()
                                {
                                    continue;
                                }
                                if let Some(ans) =
                                    constant.literal.try_eval_target_usize(tcx, param_env)
                                {
                                    current_node.const_value.push((left_ssa, ans as usize));
                                    continue;
                                }
                                if let Some(const_bool) = constant.literal.try_to_bool() {
                                    current_node
                                        .const_value
                                        .push((left_ssa, const_bool as usize));
                                }
                                continue;
                            }
                        },
                        Rvalue::Ref(_, _, ref p) => {
                            let right_ssa = p.local.as_usize();
                            if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                let right = p.clone();
                                let assign = Assignment::new(
                                    left,
                                    right,
                                    0,
                                    statement.source_info.span.clone(),
                                );
                                current_node.assignments.push(assign);
                            }
                        }
                        Rvalue::AddressOf(_, ref p) => {
                            let right_ssa = p.local.as_usize();
                            if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                let right = p.clone();
                                let assign = Assignment::new(
                                    left,
                                    right,
                                    0,
                                    statement.source_info.span.clone(),
                                );
                                current_node.assignments.push(assign);
                            }
                        }

                        Rvalue::ShallowInitBox(ref x, _) => {
                            if nodes[left_ssa].sons.contains_key(&0) == false {
                                let mut node = Node::new(left_ssa, nodes.len(), false, true);
                                let mut node1 = Node::new(left_ssa, nodes.len() + 1, false, true);
                                let mut node2 = Node::new(left_ssa, nodes.len() + 2, false, true);
                                node.alive = nodes[left_ssa].alive;
                                node1.alive = node.alive;
                                node2.alive = node.alive;
                                node.sons.insert(0, node1.local);
                                node.field_info.push(0);
                                node1.sons.insert(0, node2.local);
                                node1.field_info.push(0);
                                node1.field_info.push(0);
                                node2.field_info.push(0);
                                node2.field_info.push(0);
                                node2.field_info.push(0);
                                node2.kind = 1;
                                nodes[left_ssa].sons.insert(0, node.local);
                                nodes.push(node);
                                nodes.push(node1);
                                nodes.push(node2);
                            }
                            match x {
                                Operand::Copy(ref p) => {
                                    let right_ssa = p.local.as_usize();
                                    if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                        let right = p.clone();
                                        let assign = Assignment::new(
                                            left,
                                            right,
                                            2,
                                            statement.source_info.span.clone(),
                                        );
                                        current_node.assignments.push(assign);
                                    }
                                }
                                Operand::Move(ref p) => {
                                    let right_ssa = p.local.as_usize();
                                    if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                        let right = p.clone();
                                        let assign = Assignment::new(
                                            left,
                                            right,
                                            2,
                                            statement.source_info.span.clone(),
                                        );
                                        current_node.assignments.push(assign);
                                    }
                                }
                                Operand::Constant(_) => {}
                            }
                        }
                        Rvalue::Cast(_, ref x, _) => match x {
                            Operand::Copy(ref p) => {
                                let right_ssa = p.local.as_usize();
                                if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                    let right = p.clone();
                                    let assign = Assignment::new(
                                        left,
                                        right,
                                        0,
                                        statement.source_info.span.clone(),
                                    );
                                    current_node.assignments.push(assign);
                                }
                            }
                            Operand::Move(ref p) => {
                                let right_ssa = p.local.as_usize();
                                if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                    let right = p.clone();
                                    let assign = Assignment::new(
                                        left,
                                        right,
                                        1,
                                        statement.source_info.span.clone(),
                                    );
                                    current_node.assignments.push(assign);
                                }
                            }
                            Operand::Constant(_) => {}
                        },
                        Rvalue::Aggregate(_, ref x) => {
                            for each_x in x {
                                match each_x {
                                    Operand::Copy(ref p) => {
                                        let right_ssa = p.local.as_usize();
                                        if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                            let right = p.clone();
                                            let assign = Assignment::new(
                                                left,
                                                right,
                                                0,
                                                statement.source_info.span.clone(),
                                            );
                                            current_node.assignments.push(assign);
                                        }
                                    }
                                    Operand::Move(ref p) => {
                                        let right_ssa = p.local.as_usize();
                                        if nodes[left_ssa].so_so() && nodes[right_ssa].so_so() {
                                            let right = p.clone();
                                            let assign = Assignment::new(
                                                left,
                                                right,
                                                0,
                                                statement.source_info.span.clone(),
                                            );
                                            current_node.assignments.push(assign);
                                        }
                                    }
                                    Operand::Constant(_) => {}
                                }
                            }
                        }
                        Rvalue::Discriminant(ref p) => {
                            let right = p.clone();
                            let assign =
                                Assignment::new(left, right, 3, statement.source_info.span.clone());
                            current_node.assignments.push(assign);
                        }
                        _ => {}
                    }
                }
            }

            // handle terminator statements
            match terminator.kind {
                TerminatorKind::Goto { ref target } => {
                    current_node.push(target.as_usize());
                }
                TerminatorKind::SwitchInt {
                    discr: _,
                    ref targets,
                } => {
                    current_node.switch_stmts.push(terminator.clone());
                    for (_, ref target) in targets.iter() {
                        current_node.push(target.as_usize());
                    }
                    current_node.push(targets.otherwise().as_usize());
                }

                TerminatorKind::Return => {}
                TerminatorKind::GeneratorDrop | TerminatorKind::Unreachable => {}
                TerminatorKind::Drop {
                    place: _,
                    ref target,
                    ref unwind,
                    replace: _,
                } => {
                    current_node.push(target.as_usize());
                    current_node.drops.push(terminator.clone());
                    match unwind {
                        UnwindAction::Cleanup(tt) => {
                            current_node.push(tt.as_usize());
                        }
                        _ => {}
                    }
                }

                TerminatorKind::Call {
                    func: _,
                    args: _,
                    destination: _,
                    ref target,
                    ref unwind,
                    call_source: _,
                    fn_span: _,
                } => {
                    if let Some(tt) = target {
                        current_node.push(tt.as_usize());
                    }
                    match unwind {
                        rustc_middle::mir::UnwindAction::Cleanup(tt) => {
                            current_node.push(tt.as_usize());
                        }
                        _ => {}
                    }

                    current_node.calls.push(terminator.clone());
                }
                TerminatorKind::Assert {
                    cond: _,
                    expected: _,
                    msg: _,
                    ref target,
                    ref unwind,
                } => {
                    current_node.push(target.as_usize());
                    match unwind {
                        UnwindAction::Cleanup(tt) => {
                            current_node.push(tt.as_usize());
                        }
                        _ => {}
                    }
                }
                TerminatorKind::Yield {
                    value: _,
                    ref resume,
                    resume_arg: _,
                    ref drop,
                } => {
                    current_node.push(resume.as_usize());
                    if let Some(target) = drop {
                        current_node.push(target.as_usize());
                    }
                }
                TerminatorKind::FalseEdge {
                    ref real_target,
                    imaginary_target: _,
                } => {
                    current_node.push(real_target.as_usize());
                }
                TerminatorKind::FalseUnwind {
                    ref real_target,
                    unwind: _,
                } => {
                    current_node.push(real_target.as_usize());
                }
                TerminatorKind::InlineAsm {
                    template: _,
                    operands: _,
                    options: _,
                    line_spans: _,
                    ref destination,
                    ref unwind,
                    ..
                } => {
                    match destination {
                        Some(target) => {
                            current_node.push(target.as_usize());
                        }
                        None => {}
                    }
                    match unwind {
                        UnwindAction::Cleanup(target) => {
                            current_node.push(target.as_usize());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            blocks.push(current_node);
        }

        SafeDropGraph {
            def_id: def_id.clone(),
            span: my_body.span,
            blocks: blocks,
            nodes: nodes,
            arg_size: arg_size,
            father_block: father_block,
            constant_bool: FxHashMap::default(),
            count: 0,
            return_results: ReturnResults::new(arg_size),
            return_set: FxHashSet::default(),
            bug_records: BugRecords::new(),
            visit_times: 0,
        }
    }

    pub fn tarjan(
        &mut self,
        index: usize,
        stack: &mut Vec<usize>,
        instack: &mut FxHashSet<usize>,
        dfn: &mut Vec<usize>,
        low: &mut Vec<usize>,
    ) {
        dfn[index] = self.count;
        low[index] = self.count;
        self.count += 1;
        instack.insert(index);
        stack.push(index);
        let out_set = self.blocks[index].next.clone();
        for i in out_set {
            let target = i;
            if dfn[target] == 0 {
                self.tarjan(target, stack, instack, dfn, low);
                low[index] = min(low[index], low[target]);
            } else {
                if instack.contains(&target) {
                    low[index] = min(low[index], dfn[target]);
                }
            }
        }
        // generate SCC
        if dfn[index] == low[index] {
            loop {
                let top = stack.pop().unwrap();
                self.father_block[top] = index;
                instack.remove(&top);
                if index == top {
                    break;
                }
                let top_node = self.blocks[top].next.clone();
                for i in top_node {
                    self.blocks[index].next.insert(i);
                }
                self.blocks[index].sub_blocks.push(top);
                for i in self.blocks[top].sub_blocks.clone() {
                    self.blocks[index].sub_blocks.push(i);
                }
            }
            self.blocks[index].sub_blocks.reverse();
            //remove the out nodes which is in the current SCC
            let mut remove_list = Vec::new();
            for i in self.blocks[index].next.iter() {
                if self.father_block[*i] == index {
                    remove_list.push(*i);
                }
            }
            for i in remove_list {
                self.blocks[index].next.remove(&i);
            }
        }
    }

    // handle SCC
    pub fn solve_scc(&mut self) {
        let mut stack = Vec::<usize>::new();
        let mut instack = FxHashSet::<usize>::default();
        let mut dfn = vec![0 as usize; self.blocks.len()];
        let mut low = vec![0 as usize; self.blocks.len()];
        self.tarjan(0, &mut stack, &mut instack, &mut dfn, &mut low);
    }

    //can also use the format to check.
    //these function calls are the functions whose MIRs can not be fetched.
    pub fn corner_handle(
        &mut self,
        _left_ssa: usize,
        _merge_vec: &Vec<usize>,
        _move_set: &mut FxHashSet<usize>,
        def_id: DefId,
    ) -> bool {
        // function::call_mut
        if def_id.index.as_usize() == 3430 {
            return true;
        }
        //function::iterator::next
        if def_id.index.as_usize() == 8476 {
            return true;
        }
        //intrinsic_offset
        if def_id.index.as_usize() == 1709 {
            return true;
        }
        return false;
    }

    //the dangling pointer occuring in some functions like drop() is reasonable.
    pub fn should_check(def_id: DefId) -> bool {
        let def_str = format!("{:?}", def_id);
        if let Some(_) = def_str.find("drop") {
            return false;
        }
        if let Some(_) = def_str.find("dealloc") {
            return false;
        }
        if let Some(_) = def_str.find("release") {
            return false;
        }
        if let Some(_) = def_str.find("destroy") {
            return false;
        }
        return true;
    }

    // alias analysis for a single block
    pub fn alias_check(
        &mut self,
        bb_index: usize,
        tcx: TyCtxt<'tcx>,
        move_set: &mut FxHashSet<usize>,
    ) {
        for stmt in self.blocks[bb_index].const_value.clone() {
            self.constant_bool.insert(stmt.0, stmt.1);
        }
        let current_block = self.blocks[bb_index].clone();
        for i in current_block.assignments {
            let mut l_node_ref =
                self.handle_projection(false, i.left.local.as_usize(), tcx, i.left.clone());
            let r_node_ref =
                self.handle_projection(true, i.right.local.as_usize(), tcx, i.right.clone());
            if i.atype == 3 {
                self.nodes[l_node_ref].alias[0] = r_node_ref;
                continue;
            }
            self.uaf_check(r_node_ref, i.span, i.right.local.as_usize(), false);
            self.fill_alive(l_node_ref, self.father_block[bb_index] as isize);
            if i.atype == 2 {
                l_node_ref = *self.nodes[l_node_ref].sons.get(&0).unwrap() + 2;
                self.nodes[l_node_ref].alive = self.father_block[bb_index] as isize;
                self.nodes[l_node_ref - 1].alive = self.father_block[bb_index] as isize;
                self.nodes[l_node_ref - 2].alive = self.father_block[bb_index] as isize;
            }
            merge_alias(move_set, l_node_ref, r_node_ref, &mut self.nodes);
        }
    }

    // interprocedure alias analysis, mainly handle the function call statement
    pub fn call_alias_check(
        &mut self,
        bb_index: usize,
        tcx: TyCtxt<'tcx>,
        func_map: &mut FuncMap,
        move_set: &mut FxHashSet<usize>,
    ) {
        let current_block = self.blocks[bb_index].clone();
        for call in current_block.calls {
            if let TerminatorKind::Call {
                ref func,
                ref args,
                ref destination,
                target: _,
                unwind: _,
                call_source: _,
                fn_span: _,
            } = call.kind
            {
                if let Operand::Constant(ref constant) = func {
                    let left_ssa = self.handle_projection(
                        false,
                        destination.local.as_usize(),
                        tcx,
                        destination.clone(),
                    );
                    self.nodes[left_ssa].alive = self.father_block[bb_index] as isize;
                    let mut merge_vec = Vec::new();
                    merge_vec.push(left_ssa);
                    let mut so_so_flag = 0;
                    if self.nodes[left_ssa].so_so() {
                        so_so_flag += 1;
                    }
                    for arg in args {
                        match arg {
                            Operand::Copy(ref p) => {
                                let right_ssa = self.handle_projection(
                                    true,
                                    p.local.as_usize(),
                                    tcx,
                                    p.clone(),
                                );
                                self.uaf_check(
                                    right_ssa,
                                    call.source_info.span,
                                    p.local.as_usize(),
                                    true,
                                );
                                merge_vec.push(right_ssa);
                                if self.nodes[right_ssa].so_so() {
                                    so_so_flag += 1;
                                }
                            }
                            Operand::Move(ref p) => {
                                let right_ssa = self.handle_projection(
                                    true,
                                    p.local.as_usize(),
                                    tcx,
                                    p.clone(),
                                );
                                self.uaf_check(
                                    right_ssa,
                                    call.source_info.span,
                                    p.local.as_usize(),
                                    true,
                                );
                                merge_vec.push(right_ssa);
                                if self.nodes[right_ssa].so_so() {
                                    so_so_flag += 1;
                                }
                            }
                            Operand::Constant(_) => {
                                merge_vec.push(0);
                            }
                        }
                    }
                    if let ty::FnDef(ref target_id, _) = constant.literal.ty().kind() {
                        if so_so_flag > 1
                            || (so_so_flag > 0 && Self::should_check(target_id.clone()) == false)
                        {
                            if tcx.is_mir_available(*target_id) {
                                if func_map.map.contains_key(&target_id.index.as_usize()) {
                                    let assignments =
                                        func_map.map.get(&target_id.index.as_usize()).unwrap();
                                    for assign in assignments.assignments.iter() {
                                        if !assign.valuable() {
                                            continue;
                                        }
                                        merge(move_set, &mut self.nodes, assign, &merge_vec);
                                    }
                                    for dead in assignments.dead.iter() {
                                        let drop = merge_vec[*dead];
                                        self.dead_node(drop, 99999, &call.source_info, false);
                                    }
                                } else {
                                    if func_map.set.contains(&target_id.index.as_usize()) {
                                        continue;
                                    }
                                    func_map.set.insert(target_id.index.as_usize());
                                    let func_body = tcx.optimized_mir(*target_id);
                                    let mut safedrop_graph =
                                        SafeDropGraph::new(&func_body, tcx, *target_id);
                                    safedrop_graph.solve_scc();
                                    safedrop_graph.safedrop_check(0, tcx, func_map);
                                    let return_results = safedrop_graph.return_results.clone();
                                    for assign in return_results.assignments.iter() {
                                        if !assign.valuable() {
                                            continue;
                                        }
                                        merge(move_set, &mut self.nodes, assign, &merge_vec);
                                    }
                                    for dead in return_results.dead.iter() {
                                        let drop = merge_vec[*dead];
                                        self.dead_node(drop, 99999, &call.source_info, false);
                                    }
                                    func_map
                                        .map
                                        .insert(target_id.index.as_usize(), return_results);
                                }
                            } else {
                                if self.nodes[left_ssa].so_so() {
                                    if self
                                        .corner_handle(left_ssa, &merge_vec, move_set, *target_id)
                                    {
                                        continue;
                                    }
                                    let mut right_set = Vec::new();
                                    for right_ssa in &merge_vec {
                                        if self.nodes[*right_ssa].so_so()
                                            && left_ssa != *right_ssa
                                            && self.nodes[left_ssa].is_ptr()
                                        {
                                            right_set.push(*right_ssa);
                                        }
                                    }
                                    if right_set.len() == 1 {
                                        merge_alias(
                                            move_set,
                                            left_ssa,
                                            right_set[0],
                                            &mut self.nodes,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // analyze the drop statement and update the alive state for nodes.
    pub fn drop_check(&mut self, bb_index: usize, tcx: TyCtxt<'tcx>) {
        let current_block = self.blocks[bb_index].clone();
        for drop in current_block.drops {
            match drop.kind {
                TerminatorKind::Drop {
                    ref place,
                    target: _,
                    unwind: _,
                    replace,
                } => {
                    let life_begin = self.father_block[bb_index];
                    let drop_local =
                        self.handle_projection(false, place.local.as_usize(), tcx, place.clone());
                    let info = drop.source_info.clone();
                    self.dead_node(drop_local, life_begin, &info, false);
                }
                _ => {}
            }
        }
    }

    // the core function of the safedrop.
    pub fn safedrop_check(&mut self, bb_index: usize, tcx: TyCtxt<'tcx>, func_map: &mut FuncMap) {
        self.visit_times += 1;
        if self.visit_times > 10000 {
            return;
        }
        let current_block = self.blocks[self.father_block[bb_index]].clone();
        let mut move_set = FxHashSet::default();
        self.alias_check(self.father_block[bb_index], tcx, &mut move_set);
        self.call_alias_check(self.father_block[bb_index], tcx, func_map, &mut move_set);
        self.drop_check(self.father_block[bb_index], tcx);
        if current_block.sub_blocks.len() > 0 {
            for i in current_block.sub_blocks.clone() {
                self.alias_check(i, tcx, &mut move_set);
                self.call_alias_check(i, tcx, func_map, &mut move_set);
                self.drop_check(i, tcx);
            }
        }

        //finish the analysis for a path
        if current_block.next.len() == 0 {
            // check the bugs.
            if Self::should_check(self.def_id) {
                self.bug_check(&current_block);
            }
            // merge the result.
            let results_nodes = self.nodes.clone();
            self.merge_results(results_nodes, current_block.is_cleanup);
        }

        //search for the next block to visit.
        let mut loop_flag = true;
        let mut ans_bool = 0;
        let mut s_target = 0;
        let mut discr_target = 0;
        let mut s_targets = None;
        //handle the SwitchInt statement.
        if current_block.switch_stmts.is_empty() == false && current_block.sub_blocks.is_empty() {
            if let TerminatorKind::SwitchInt {
                ref discr,

                ref targets,
            } = current_block.switch_stmts[0].clone().kind
            {
                if let Some(p) = discr.place() {
                    let place = self.handle_projection(false, p.local.as_usize(), tcx, p.clone());
                    if let Some(const_bool) = self.constant_bool.get(&self.nodes[place].alias[0]) {
                        loop_flag = false;
                        ans_bool = *const_bool;
                    }
                    if self.nodes[place].alias[0] != place {
                        discr_target = self.nodes[place].alias[0];
                        s_targets = Some(targets.clone());
                    }
                } else {
                    loop {
                        if let None = discr.constant() {
                            break;
                        }
                        let temp = discr.constant().unwrap().literal;
                        if let None = temp.try_to_scalar() {
                            break;
                        }
                        if let Err(_tmp) = temp.try_to_scalar().clone().unwrap().try_to_int() {
                            break;
                        }
                        let param_env = tcx.param_env(self.def_id);
                        if let Some(const_bool) = temp.try_eval_target_usize(tcx, param_env) {
                            loop_flag = false;
                            ans_bool = const_bool as usize;
                            break;
                        }
                        if let Some(const_bool) = temp.try_to_bool() {
                            loop_flag = false;
                            ans_bool = const_bool as usize;
                        }
                        break;
                    }
                }
                if !loop_flag {
                    for iter in targets.iter() {
                        if iter.0 as usize == ans_bool as usize {
                            s_target = iter.1.as_usize();
                            break;
                        }
                    }
                    if s_target == 0 {
                        let all_target = targets.all_targets();
                        if ans_bool as usize >= all_target.len() {
                            s_target = all_target[all_target.len() - 1].as_usize();
                        } else {
                            s_target = all_target[ans_bool as usize].as_usize();
                        }
                    }
                }
            }
        }
        // only one path
        if current_block.next.len() == 1 {
            for next_index in current_block.next {
                self.safedrop_check(next_index, tcx, func_map);
            }
        } else {
            // fixed path since a constant switchInt value
            if loop_flag == false {
                self.safedrop_check(s_target, tcx, func_map);
            } else {
                // Other cases in switchInt terminators
                if let Some(targets) = s_targets {
                    for iter in targets.iter() {
                        if self.visit_times > 10000 {
                            continue;
                        }
                        let next_index = iter.1.as_usize();
                        let backup_nodes = self.nodes.clone();
                        let constant_record = self.constant_bool.clone();
                        self.constant_bool.insert(discr_target, iter.0 as usize);
                        self.safedrop_check(next_index, tcx, func_map);
                        self.nodes = backup_nodes;
                        self.constant_bool = constant_record;
                    }
                    let all_targets = targets.all_targets();
                    let next_index = all_targets[all_targets.len() - 1].as_usize();
                    let backup_nodes = self.nodes.clone();
                    let constant_record = self.constant_bool.clone();
                    self.constant_bool.insert(discr_target, 99999 as usize);
                    self.safedrop_check(next_index, tcx, func_map);
                    self.nodes = backup_nodes;
                    self.constant_bool = constant_record;
                } else {
                    for i in current_block.next {
                        if self.visit_times > 10000 {
                            continue;
                        }
                        let next_index = i;
                        let backup_nodes = self.nodes.clone();
                        let constant_record = self.constant_bool.clone();
                        self.safedrop_check(next_index, tcx, func_map);
                        self.nodes = backup_nodes;
                        self.constant_bool = constant_record;
                    }
                }
            }
        }
    }

    pub fn output_warning(&self) {
        if self.bug_records.is_bug_free() {
            return;
        }
        println!("=================================");
        println!("Function:{0:?};{1:?}", self.def_id, self.def_id.index);
        self.bug_records.df_bugs_output();
        self.bug_records.uaf_bugs_output();
        self.bug_records.dp_bug_output(self.span);

        println!();
        println!();
    }

    // assign to the variable _x, we will set the alive of _x and its child nodes a new alive.
    pub fn fill_alive(&mut self, node: usize, alive: isize) {
        self.nodes[node].alive = alive;
        //TODO: check the correctness.
        for i in self.nodes[node].alias.clone() {
            if self.nodes[i].alive == -1 {
                self.nodes[i].alive = alive;
            }
        }
        for i in self.nodes[node].sons.clone().into_iter() {
            self.fill_alive(i.1, alive);
        }
    }

    pub fn exist_dead(&self, node: usize, record: &mut FxHashSet<usize>, dangling: bool) -> bool {
        //if is a dangling pointer check, only check the pointer type varible.
        if self.nodes[node].is_alive() == false
            && (dangling && self.nodes[node].is_ptr() || !dangling)
        {
            return true;
        }
        record.insert(node);
        if self.nodes[node].alias[0] != node {
            for i in self.nodes[node].alias.clone().into_iter() {
                if i != node && record.contains(&i) == false && self.exist_dead(i, record, dangling)
                {
                    return true;
                }
            }
        }
        for i in self.nodes[node].sons.clone().into_iter() {
            if record.contains(&i.1) == false && self.exist_dead(i.1, record, dangling) {
                return true;
            }
        }
        return false;
    }

    pub fn df_check(&mut self, drop: usize, span: Span) -> bool {
        let root = self.nodes[drop].index;
        if self.nodes[drop].is_alive() == false
            && self.bug_records.df_bugs.contains_key(&root) == false
        {
            self.bug_records.df_bugs.insert(root, span.clone());
        }
        return self.nodes[drop].is_alive() == false;
    }

    pub fn uaf_check(&mut self, used: usize, span: Span, origin: usize, is_func_call: bool) {
        let mut record = FxHashSet::default();
        if self.nodes[used].so_so()
            && (!self.nodes[used].is_ptr() || self.nodes[used].index != origin || is_func_call)
            && self.exist_dead(used, &mut record, false) == true
            && self.bug_records.uaf_bugs.contains(&span) == false
        {
            self.bug_records.uaf_bugs.insert(span.clone());
        }
    }

    pub fn dp_check(&self, local: usize) -> bool {
        let mut record = FxHashSet::default();
        return self.exist_dead(local, &mut record, local != 0);
    }

    pub fn bug_check(&mut self, current_block: &BlockNode<'tcx>) {
        if current_block.is_cleanup == false {
            if self.nodes[0].so_so() && self.dp_check(0) {
                self.bug_records.dp_bug = true;
            } else {
                for i in 0..self.arg_size {
                    if self.nodes[i + 1].is_ptr() && self.dp_check(i + 1) {
                        self.bug_records.dp_bug = true;
                    }
                }
            }
        } else {
            for i in 0..self.arg_size {
                if self.nodes[i + 1].is_ptr() && self.dp_check(i + 1) {
                    self.bug_records.dp_bug_unwind = true;
                }
            }
        }
    }

    pub fn dead_node(&mut self, drop: usize, life_begin: usize, info: &SourceInfo, alias: bool) {
        //Rc drop
        if self.nodes[drop].is_corner_case() {
            return;
        }
        //check if there is a double free bug.
        if self.df_check(drop, info.span) {
            return;
        }
        //drop their alias
        if self.nodes[drop].alias[0] != drop {
            for i in self.nodes[drop].alias.clone().into_iter() {
                if self.nodes[i].is_ref() {
                    continue;
                }
                self.dead_node(i, life_begin, info, true);
            }
        }
        //drop the sons of the root node.
        //alias flag is used to avoid the sons of the alias are dropped repeatly.
        if alias == false {
            for i in self.nodes[drop].sons.clone().into_iter() {
                if self.nodes[drop].is_tuple() == true && self.nodes[i.1].need_drop() == false {
                    continue;
                }
                self.dead_node(i.1, life_begin, info, false);
            }
        }
        //SCC.
        if self.nodes[drop].alive < life_begin as isize && self.nodes[drop].so_so() {
            self.nodes[drop].dead();
        }
    }

    // field-sensitive fetch instruction for a variable.
    // is_right: 2 = 1.0; 0 = 2.0; => 0 = 1.0.0;
    pub fn handle_projection(
        &mut self,
        is_right: bool,
        local: usize,
        tcx: TyCtxt<'tcx>,
        place: Place<'tcx>,
    ) -> usize {
        let mut init_local = local;
        let mut current_local = local;
        for projection in place.projection {
            match projection {
                ProjectionElem::Deref => {
                    if current_local == self.nodes[current_local].alias[0]
                        && self.nodes[current_local].is_ref() == false
                    {
                        let need_drop = true;
                        let so_so = true;
                        let mut node = Node::new(
                            self.nodes.len(),
                            self.nodes.len(),
                            need_drop,
                            need_drop || !so_so,
                        );
                        node.kind = 1; //TODO
                        node.alive = self.nodes[current_local].alive;
                        self.nodes[current_local].alias[0] = self.nodes.len();
                        self.nodes.push(node);
                    }
                    current_local = self.nodes[current_local].alias[0];
                    init_local = self.nodes[current_local].index;
                }
                ProjectionElem::Field(field, ty) => {
                    let index = field.as_usize();
                    if is_right && self.nodes[current_local].alias[0] != current_local {
                        current_local = self.nodes[current_local].alias[0];
                        init_local = self.nodes[current_local].index;
                    }
                    if self.nodes[current_local].sons.contains_key(&index) == false {
                        let param_env = tcx.param_env(self.def_id);
                        let need_drop = ty.needs_drop(tcx, param_env);
                        let so_so = so_so(ty);
                        let mut node =
                            Node::new(init_local, self.nodes.len(), need_drop, need_drop || !so_so);
                        node.kind = kind(ty);
                        node.alive = self.nodes[current_local].alive;
                        node.field_info = self.nodes[current_local].field_info.clone();
                        node.field_info.push(index);
                        self.nodes[current_local].sons.insert(index, node.local);
                        self.nodes.push(node);
                    }
                    current_local = *self.nodes[current_local].sons.get(&index).unwrap();
                }
                _ => {}
            }
        }
        return current_local;
    }

    //merge the result of current path to the final result.
    pub fn merge_results(&mut self, results_nodes: Vec<Node>, is_cleanup: bool) {
        for node in results_nodes.iter() {
            if node.index <= self.arg_size {
                if node.alias[0] != node.local || node.alias.len() > 1 {
                    for alias in node.alias.clone() {
                        if results_nodes[alias].index <= self.arg_size
                            && !self.return_set.contains(&(node.local, alias))
                            && alias != node.local
                            && node.index != results_nodes[alias].index
                        {
                            self.return_set.insert((node.local, alias));
                            let left_node = node;
                            let right_node = &results_nodes[alias];
                            let mut new_assign = ReturnAssign::new(
                                0,
                                left_node.index,
                                left_node.so_so(),
                                left_node.need_drop(),
                                right_node.index,
                                right_node.so_so(),
                                right_node.need_drop(),
                            );
                            new_assign.left = left_node.field_info.clone();
                            new_assign.right = right_node.field_info.clone();
                            self.return_results.assignments.push(new_assign);
                        }
                    }
                }
                if node.is_ptr()
                    && is_cleanup == false
                    && node.is_alive() == false
                    && node.local <= self.arg_size
                {
                    self.return_results.dead.insert(node.local);
                }
            }
        }
    }
}

//these adt structs use the Rc-kind drop instruction, which we do not focus on.
pub fn is_corner_adt(str: String) -> bool {
    if let Some(_) = str.find("cell::RefMut") {
        return true;
    }
    if let Some(_) = str.find("cell::Ref") {
        return true;
    }
    if let Some(_) = str.find("rc::Rc") {
        return true;
    }
    return false;
}

pub fn kind<'tcx>(current_ty: Ty<'tcx>) -> usize {
    match current_ty.kind() {
        ty::RawPtr(..) => 1,
        ty::Ref(..) => 4,
        ty::Tuple(..) => 2,
        ty::Adt(ref adt_def, _) => {
            if is_corner_adt(format!("{:?}", adt_def)) {
                return 3;
            } else {
                return 0;
            }
        }
        _ => 0,
    }
}

//type filter.
pub fn so_so<'tcx>(current_ty: Ty<'tcx>) -> bool {
    match current_ty.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => true,
        ty::Array(ref tys, _) => so_so(*tys),
        ty::Adt(_, ref substs) => {
            for tys in substs.types() {
                if !so_so(tys) {
                    return false;
                }
            }
            true
        }
        ty::Tuple(ref substs) => {
            for tys in substs.iter() {
                if !so_so(tys) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

//instruction to assign alias for a variable.
pub fn merge_alias(
    move_set: &mut FxHashSet<usize>,
    left_ssa: usize,
    right_ssa: usize,
    nodes: &mut Vec<Node>,
) {
    if nodes[left_ssa].index == nodes[right_ssa].index {
        return;
    }
    if move_set.contains(&left_ssa) {
        let mut alias_clone = nodes[right_ssa].alias.clone();
        nodes[left_ssa].alias.append(&mut alias_clone);
    } else {
        move_set.insert(left_ssa);
        nodes[left_ssa].alias = nodes[right_ssa].alias.clone();
    }
    for son in nodes[right_ssa].sons.clone().into_iter() {
        if nodes[left_ssa].sons.contains_key(&son.0) == false {
            let mut node = Node::new(
                nodes[left_ssa].index,
                nodes.len(),
                nodes[son.1].need_drop(),
                nodes[son.1].so_so(),
            );
            node.kind = nodes[son.1].kind;
            node.alive = nodes[left_ssa].alive;
            node.field_info = nodes[left_ssa].field_info.clone();
            node.field_info.push(son.0);
            nodes[left_ssa].sons.insert(son.0, node.local);
            nodes.push(node);
        }
        let l_son = *(nodes[left_ssa].sons.get(&son.0).unwrap());
        merge_alias(move_set, l_son, son.1, nodes);
    }
}

//inter-procedure instruction to merge alias.
pub fn merge(
    move_set: &mut FxHashSet<usize>,
    nodes: &mut Vec<Node>,
    assign: &ReturnAssign,
    arg_vec: &Vec<usize>,
) {
    if assign.left_index >= arg_vec.len() {
        println!("vector warning!");
        return;
    }
    if assign.right_index >= arg_vec.len() {
        println!("vector warning!");
        return;
    }
    let left_init = arg_vec[assign.left_index];
    let mut right_init = arg_vec[assign.right_index];
    let mut left_ssa = left_init;
    let mut right_ssa = right_init;
    for index in assign.left.iter() {
        if nodes[left_ssa].sons.contains_key(&index) == false {
            let need_drop = assign.left_need_drop;
            let so_so = assign.left_so_so;
            let mut node = Node::new(left_init, nodes.len(), need_drop, so_so);
            node.kind = 1;
            node.alive = nodes[left_ssa].alive;
            node.field_info = nodes[left_ssa].field_info.clone();
            node.field_info.push(*index);
            nodes[left_ssa].sons.insert(*index, node.local);
            nodes.push(node);
        }
        left_ssa = *nodes[left_ssa].sons.get(&index).unwrap();
    }
    for index in assign.right.iter() {
        if nodes[right_ssa].alias[0] != right_ssa {
            right_ssa = nodes[right_ssa].alias[0];
            right_init = nodes[right_ssa].index;
        }
        if nodes[right_ssa].sons.contains_key(&index) == false {
            let need_drop = assign.right_need_drop;
            let so_so = assign.right_so_so;
            let mut node = Node::new(right_init, nodes.len(), need_drop, so_so);
            node.kind = 1;
            node.alive = nodes[right_ssa].alive;
            node.field_info = nodes[right_ssa].field_info.clone();
            node.field_info.push(*index);
            nodes[right_ssa].sons.insert(*index, node.local);
            nodes.push(node);
        }
        right_ssa = *nodes[right_ssa].sons.get(&index).unwrap();
    }
    merge_alias(move_set, left_ssa, right_ssa, nodes);
}

//struct to cache the results for analyzed functions.
#[derive(Clone)]
pub struct FuncMap {
    pub map: FxHashMap<usize, ReturnResults>,
    pub set: FxHashSet<usize>,
}

impl FuncMap {
    pub fn new() -> FuncMap {
        FuncMap {
            map: FxHashMap::default(),
            set: FxHashSet::default(),
        }
    }
}

//structure to record the existed bugs.
pub struct BugRecords {
    pub df_bugs: FxHashMap<usize, Span>,
    pub df_bugs_unwind: FxHashMap<usize, Span>,
    pub uaf_bugs: FxHashSet<Span>,
    pub dp_bug: bool,
    pub dp_bug_unwind: bool,
}

impl BugRecords {
    pub fn new() -> BugRecords {
        BugRecords {
            df_bugs: FxHashMap::default(),
            df_bugs_unwind: FxHashMap::default(),
            uaf_bugs: FxHashSet::default(),
            dp_bug: false,
            dp_bug_unwind: false,
        }
    }

    pub fn is_bug_free(&self) -> bool {
        return self.df_bugs.is_empty()
            && self.uaf_bugs.is_empty()
            && self.dp_bug == false
            && self.dp_bug_unwind == false;
    }

    pub fn df_bugs_output(&self) {
        if self.df_bugs.is_empty() {
            return;
        }
        println!("Double Free Bugs Exist:");
        for i in self.df_bugs.iter() {
            println!("occurs in {:?}", i.1);
        }
    }

    pub fn uaf_bugs_output(&self) {
        if self.uaf_bugs.is_empty() {
            return;
        }
        println!("Use After Free Bugs Exist:");
        for i in self.uaf_bugs.iter() {
            println!("occurs in {:?}", i);
        }
    }

    pub fn dp_bug_output(&self, span: Span) {
        if self.dp_bug {
            println!("Dangling Pointer Bug Exist {:?}", span);
        }
        if self.dp_bug_unwind {
            println!("Dangling Pointer Bug Exist in Unwinding {:?}", span);
        }
    }
}
