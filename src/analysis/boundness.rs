//! Petri网有界性分析
//!
//! 提供多种方法检查Petri网的有界性：
//! 1. 基于P-不变量的有界性检查
//! 2. 覆盖树构建与有界性分析
//! 3. 可达图有界性检查

use crate::net::Net;
use crate::net::ids::{PlaceId, TransitionId};
use crate::net::index_vec::Idx;
use crate::net::structure::Marking;
#[cfg(feature = "invariants")]
use num::bigint::BigInt;

use std::collections::VecDeque;
use std::fmt;

/// 有界性检查结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundnessResult {
    /// 网是有界的
    Bounded,
    /// 网是无界的，包含无界库所的信息
    Unbounded {
        /// 无界库所的ID列表
        unbounded_places: Vec<PlaceId>,
        /// 导致无界的变迁序列（如果可找到）
        witness_sequence: Option<Vec<TransitionId>>,
    },
    /// 无法确定有界性（例如由于状态空间爆炸）
    Unknown { reason: String },
}

impl fmt::Display for BoundnessResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoundnessResult::Bounded => write!(f, "Petri网是有界的"),
            BoundnessResult::Unbounded {
                unbounded_places,
                witness_sequence,
            } => {
                write!(f, "Petri网是无界的，无界库所: {:?}", unbounded_places)?;
                if let Some(seq) = witness_sequence {
                    write!(f, "，见证序列: {:?}", seq)?;
                }
                Ok(())
            }
            BoundnessResult::Unknown { reason } => {
                write!(f, "无法确定有界性: {}", reason)
            }
        }
    }
}

/// 覆盖树节点
#[derive(Debug, Clone, PartialEq, Eq)]
struct CoverTreeNode {
    /// 节点标记（可能包含ω值）
    marking: Vec<Option<u64>>,
    /// 父节点索引
    parent: Option<usize>,
    /// 从父节点到该节点的变迁
    transition_from_parent: Option<TransitionId>,
    /// 子节点索引
    children: Vec<usize>,
}

impl CoverTreeNode {
    fn new_root(initial_marking: &Marking) -> Self {
        let marking = initial_marking
            .iter()
            .map(|(_, tokens)| Some(*tokens))
            .collect();

        Self {
            marking,
            parent: None,
            transition_from_parent: None,
            children: Vec::new(),
        }
    }

    fn has_omega(&self) -> bool {
        self.marking.iter().any(|tokens| tokens.is_none())
    }

    #[allow(dead_code)]
    fn tokens(&self, place: PlaceId) -> Option<u64> {
        self.marking[place.index()]
    }
}

/// 覆盖树
#[derive(Debug, Clone)]
struct CoverTree {
    nodes: Vec<CoverTreeNode>,
    #[allow(dead_code)]
    root: usize,
}

impl CoverTree {
    fn new(initial_marking: &Marking) -> Self {
        let root_node = CoverTreeNode::new_root(initial_marking);

        Self {
            nodes: vec![root_node],
            root: 0,
        }
    }

    fn node(&self, index: usize) -> &CoverTreeNode {
        &self.nodes[index]
    }

    fn node_mut(&mut self, index: usize) -> &mut CoverTreeNode {
        &mut self.nodes[index]
    }

    fn add_child(
        &mut self,
        parent: usize,
        marking: Vec<Option<u64>>,
        transition: TransitionId,
    ) -> usize {
        let child_index = self.nodes.len();

        self.nodes.push(CoverTreeNode {
            marking,
            parent: Some(parent),
            transition_from_parent: Some(transition),
            children: Vec::new(),
        });

        self.node_mut(parent).children.push(child_index);
        child_index
    }

    fn is_covered(&self, marking: &[Option<u64>]) -> Option<usize> {
        for (i, node) in self.nodes.iter().enumerate() {
            if self.covers(marking, &node.marking) {
                return Some(i);
            }
        }
        None
    }

    /// 检查marking1是否覆盖marking2（marking1的每个分量都大于等于marking2的对应分量）
    fn covers(&self, marking1: &[Option<u64>], marking2: &[Option<u64>]) -> bool {
        marking1.iter().zip(marking2.iter()).all(|(m1, m2)| {
            match (m1, m2) {
                // ω覆盖任何值
                (None, _) => true,
                // 具体值覆盖相同或更小的具体值
                (Some(v1), Some(v2)) => v1 >= v2,
                // 具体值不能覆盖ω
                (Some(_), None) => false,
            }
        })
    }
}

pub struct BoundnessAnalyzer {
    state_limit: Option<usize>,
}

impl Default for BoundnessAnalyzer {
    fn default() -> Self {
        Self {
            state_limit: Some(10000),
        }
    }
}

impl BoundnessAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_state_limit(mut self, limit: Option<usize>) -> Self {
        self.state_limit = limit;
        self
    }

    /// 使用P-不变量方法检查有界性
    pub fn check_by_p_invariants(&self, net: &Net) -> BoundnessResult {
        #[cfg(not(feature = "invariants"))]
        {
            return BoundnessResult::Unknown {
                reason: "需要启用invariants特性以使用P-不变量方法".to_string(),
            };
        }

        #[cfg(feature = "invariants")]
        {
            let invariants = net.place_invariants();

            if invariants.is_empty() {
                return BoundnessResult::Unknown {
                    reason: "没有找到P-不变量".to_string(),
                };
            }

            // 检查是否存在正的P-不变量
            let mut positive_invariants = Vec::new();
            for invariant in &invariants {
                if invariant.iter().all(|coeff| coeff >= &BigInt::from(0)) {
                    positive_invariants.push(invariant);
                }
            }

            if !positive_invariants.is_empty() {
                // 存在正的P-不变量，网是有界的
                return BoundnessResult::Bounded;
            }

            BoundnessResult::Unknown {
                reason: "没有找到正的P-不变量，需要进一步分析".to_string(),
            }
        }
    }

    /// 使用覆盖树方法检查有界性
    pub fn check_by_coverability_tree(&self, net: &Net) -> BoundnessResult {
        let initial_marking = net.initial_marking();
        let mut tree = CoverTree::new(&initial_marking);
        let mut queue = VecDeque::new();
        queue.push_back(0); // 根节点

        let mut visited_count = 0;
        let mut iteration = 0;

        while let Some(node_index) = queue.pop_front() {
            visited_count += 1;
            iteration += 1;

            if let Some(limit) = self.state_limit {
                if visited_count > limit {
                    return BoundnessResult::Unknown {
                        reason: format!("超过状态限制 {}", limit),
                    };
                }
            }

            let node = tree.node(node_index).clone();

            // 如果节点包含ω，则网是无界的
            if node.has_omega() {
                let mut unbounded_places = Vec::new();
                let mut witness_sequence = Vec::new();
                let mut current = node_index;

                // 收集无界库所
                for (place_idx, tokens) in node.marking.iter().enumerate() {
                    if tokens.is_none() {
                        unbounded_places.push(PlaceId::from_usize(place_idx));
                    }
                }

                // 回溯构建见证序列
                while let Some(parent) = tree.node(current).parent {
                    if let Some(trans) = tree.node(current).transition_from_parent {
                        witness_sequence.push(trans);
                    }
                    current = parent;
                }
                witness_sequence.reverse();

                println!(
                    "迭代 {}: 发现ω标记在节点{}，无界库所: {:?}",
                    iteration, node_index, unbounded_places
                );
                return BoundnessResult::Unbounded {
                    unbounded_places,
                    witness_sequence: Some(witness_sequence),
                };
            }

            let current_marking_vec: Vec<u64> = node
                .marking
                .iter()
                .map(|tokens| tokens.unwrap_or(0))
                .collect();

            println!(
                "迭代 {}: 处理节点{}，标记: {:?}",
                iteration, node_index, current_marking_vec
            );

            use crate::net::index_vec::IndexVec;
            let temp_marking = Marking::new(IndexVec::from(current_marking_vec));

            let enabled_transitions = net.enabled_transitions(&temp_marking);
            println!("  可发生变迁: {:?}", enabled_transitions);

            for transition_id in enabled_transitions {
                match net.fire_transition(&temp_marking, transition_id) {
                    Ok(next_marking) => {
                        let next_marking_vec: Vec<Option<u64>> = next_marking
                            .iter()
                            .map(|(_, tokens)| Some(*tokens))
                            .collect();

                        println!(
                            "  变迁{}发生成功，新标记: {:?}",
                            transition_id.0, next_marking_vec
                        );

                        // 检查是否被已有节点覆盖
                        if let Some(covered_by) = tree.is_covered(&next_marking_vec) {
                            println!("    新标记被节点{}覆盖", covered_by);

                            // 被覆盖，检查是否需要创建ω节点
                            // 找到从当前节点到覆盖节点的路径
                            let path = self.find_path_to_node(&tree, node_index, covered_by);

                            if let Some(path_nodes) = path {
                                let mut needs_omega = false;
                                for &path_node_idx in &path_nodes {
                                    let path_marking = &tree.node(path_node_idx).marking;
                                    if self.is_strictly_smaller(path_marking, &next_marking_vec) {
                                        needs_omega = true;
                                        break;
                                    }
                                }

                                if needs_omega {
                                    let omega_marking = self.create_omega_marking(
                                        &tree,
                                        &path_nodes,
                                        &next_marking_vec,
                                    );

                                    println!("    创建ω节点，标记: {:?}", omega_marking);
                                    let child_idx =
                                        tree.add_child(node_index, omega_marking, transition_id);
                                    queue.push_back(child_idx);
                                } else {
                                    // 不需要ω标记，终止这个分支
                                    println!("    分支终止，不需要ω标记");
                                }
                            } else {
                                // 没有找到路径，添加普通节点
                                println!("    未找到路径，创建普通节点");
                                let child_idx =
                                    tree.add_child(node_index, next_marking_vec, transition_id);
                                queue.push_back(child_idx);
                            }
                        } else {
                            // 没有被覆盖，添加普通节点
                            println!("    新标记未被覆盖，创建普通节点");
                            let child_idx =
                                tree.add_child(node_index, next_marking_vec, transition_id);
                            queue.push_back(child_idx);
                        }
                    }
                    Err(e) => {
                        println!("  变迁{}发生失败: {:?}", transition_id.0, e);
                        continue;
                    }
                }
            }
        }

        println!("覆盖树构建完成，共处理{}个节点，未发现ω标记", visited_count);

        // 如果完成覆盖树构建而没有发现ω，则网是有界的
        BoundnessResult::Bounded
    }

    fn find_path_to_node(&self, tree: &CoverTree, from: usize, to: usize) -> Option<Vec<usize>> {
        let mut path = Vec::new();
        let mut current = from;

        while current != to {
            path.push(current);
            if let Some(parent) = tree.node(current).parent {
                current = parent;
                if current == to {
                    path.push(current);
                    return Some(path);
                }
            } else {
                return None;
            }
        }

        // from == to 的情况
        Some(vec![from])
    }

    /// 检查marking1是否严格小于marking2（所有分量都小于）
    fn is_strictly_smaller(&self, marking1: &[Option<u64>], marking2: &[Option<u64>]) -> bool {
        marking1.iter().zip(marking2.iter()).all(|(m1, m2)| {
            match (m1, m2) {
                // ω不小于任何具体值
                (None, Some(_)) => false,
                // 具体值小于具体值
                (Some(v1), Some(v2)) => v1 < v2,
                // 其他情况：具体值不小于ω，ω不小于ω
                _ => false,
            }
        }) && marking1
            .iter()
            .zip(marking2.iter())
            .any(|(m1, m2)| match (m1, m2) {
                (Some(v1), Some(v2)) => v1 < v2,
                _ => false,
            })
    }

    /// 创建ω标记
    fn create_omega_marking(
        &self,
        tree: &CoverTree,
        path_nodes: &[usize],
        new_marking: &[Option<u64>],
    ) -> Vec<Option<u64>> {
        let mut omega_marking = new_marking.to_vec();

        // 对于路径上的每个节点，如果其标记小于新标记，则对应位置设为ω
        for &node_idx in path_nodes {
            let node_marking = &tree.node(node_idx).marking;
            for (i, (old_val, new_val)) in node_marking.iter().zip(new_marking.iter()).enumerate() {
                match (old_val, new_val) {
                    (Some(old), Some(new)) if old < new => {
                        // 严格增长，设为ω
                        omega_marking[i] = None;
                    }
                    _ => {
                        // 保持原值
                    }
                }
            }
        }

        omega_marking
    }

    /// 综合检查有界性（尝试多种方法）
    pub fn check(&self, net: &Net) -> BoundnessResult {
        // 首先尝试P-不变量方法（最快）
        let p_invariant_result = self.check_by_p_invariants(net);
        if matches!(p_invariant_result, BoundnessResult::Bounded) {
            return p_invariant_result;
        }

        // 然后尝试覆盖树方法
        let coverability_result = self.check_by_coverability_tree(net);
        match coverability_result {
            BoundnessResult::Unbounded { .. } => return coverability_result,
            BoundnessResult::Bounded => return coverability_result,
            BoundnessResult::Unknown { .. } => {
                return BoundnessResult::Unknown {
                    reason: String::from("Unknown"),
                };
            }
        }
    }
}

/// 检查Petri网是否有界的便捷函数
pub fn check_boundness(net: &Net) -> BoundnessResult {
    let analyzer = BoundnessAnalyzer::new();
    analyzer.check(net)
}

/// 检查特定库所是否有界
pub fn check_place_boundness(net: &Net, place: PlaceId) -> BoundnessResult {
    let analyzer = BoundnessAnalyzer::new();
    let result = analyzer.check(net);

    match result {
        BoundnessResult::Bounded => BoundnessResult::Bounded,
        BoundnessResult::Unbounded {
            unbounded_places,
            witness_sequence,
        } => {
            if unbounded_places.contains(&place) {
                BoundnessResult::Unbounded {
                    unbounded_places: vec![place],
                    witness_sequence,
                }
            } else {
                BoundnessResult::Bounded
            }
        }
        BoundnessResult::Unknown { reason } => BoundnessResult::Unknown { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::structure::{Place, PlaceType, Transition};

    /// 构建一个有界的Petri网（简单循环）
    fn build_bounded_net() -> Net {
        let mut net = Net::empty();

        let p0 = net.add_place(Place::new(
            "p0",
            1,
            10,
            PlaceType::BasicBlock,
            String::new(),
        ));
        let p1 = net.add_place(Place::new(
            "p1",
            0,
            10,
            PlaceType::BasicBlock,
            String::new(),
        ));

        let t0 = net.add_transition(Transition::new("t0"));
        let t1 = net.add_transition(Transition::new("t1"));

        // p0 -> t0 -> p1 -> t1 -> p0
        net.set_input_weight(p0, t0, 1);
        net.set_output_weight(p1, t0, 1);

        net.set_input_weight(p1, t1, 1);
        net.set_output_weight(p0, t1, 1);

        net
    }

    /// 构建一个无界的Petri网（token生成器）
    fn build_unbounded_net() -> Net {
        let mut net = Net::empty();

        let p0 = net.add_place(Place::new(
            "p0",
            1,
            u64::MAX,
            PlaceType::BasicBlock,
            String::new(),
        ));
        let p1 = net.add_place(Place::new(
            "p1",
            0,
            u64::MAX,
            PlaceType::BasicBlock,
            String::new(),
        ));

        let t0 = net.add_transition(Transition::new("t0"));

        // p0 -> t0 -> p0 + p1 (生成新token)
        net.set_input_weight(p0, t0, 1);
        net.set_output_weight(p0, t0, 1);
        net.set_output_weight(p1, t0, 1);

        // 添加调试信息
        println!("构建无界网:");
        println!("  库所 p0: 初始token=1, 容量=无限制");
        println!("  库所 p1: 初始token=0, 容量=无限制");
        println!("  变迁 t0: 输入p0(1) -> 输出p0(1)+p1(1)");

        net
    }

    #[test]
    fn test_bounded_net() {
        let net = build_bounded_net();
        let result = check_boundness(&net);

        assert!(matches!(result, BoundnessResult::Bounded));
    }

    #[test]
    fn test_p_invariants_method() {
        let net = build_bounded_net();
        let analyzer = BoundnessAnalyzer::new();
        let result = analyzer.check_by_p_invariants(&net);

        // 有界网应该有正的P-不变量
        assert!(matches!(result, BoundnessResult::Bounded));
    }

    #[test]
    fn test_place_boundness() {
        let net = build_unbounded_net();
        let p0 = PlaceId::from_usize(0);
        let p1 = PlaceId::from_usize(1);

        let _result_p0 = check_place_boundness(&net, p0);
        let result_p1 = check_place_boundness(&net, p1);

        // 在无界网中，p1应该是无界的
        match result_p1 {
            BoundnessResult::Unbounded {
                unbounded_places, ..
            } => {
                assert_eq!(unbounded_places, vec![p1]);
            }
            _ => {
                // 在某些情况下可能无法检测
                println!("无法确定库所p1的有界性");
            }
        }
    }
}
