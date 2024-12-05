use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;
use std::hash::Hasher;

use super::petri_net::{PetriNetEdge, PetriNetNode};

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct StateEdge {
    pub label: String,
    pub weight: u32,
}

impl StateEdge {
    pub fn new(label: String, weight: u32) -> Self {
        Self { label, weight }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateNode {
    pub mark: Vec<(usize, usize)>,
}

impl Hash for StateNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut sorted_mark = self.mark.clone();
        sorted_mark.sort(); // 确保排序后再计算哈希值
        sorted_mark.hash(state);
    }
}

impl Eq for StateNode {}

impl StateNode {
    pub fn new(mark: Vec<(usize, usize)>) -> Self {
        Self { mark }
    }
}

// 规范化状态表示
pub fn normalize_state(mark: &HashSet<(NodeIndex, usize)>) -> Vec<(usize, usize)> {
    let mut state: Vec<(usize, usize)> = mark.iter().map(|(n, t)| (n.index(), *t)).collect();
    state.sort();
    state
}

fn insert_with_comparison(
    set: &mut HashSet<Vec<(usize, usize)>>,
    value: &Vec<(usize, usize)>,
) -> bool {
    for existing_value in set.iter() {
        if existing_value == value {
            return false;
        }
    }
    set.insert(value.clone());
    return true;
}

#[derive(Debug, Clone)]
pub struct StateGraph {
    pub graph: Graph<StateNode, StateEdge>,
    initial_net: Box<Graph<PetriNetNode, PetriNetEdge>>,
    initial_mark: HashSet<(NodeIndex, usize)>,
    deadlock_marks: HashSet<Vec<(usize, usize)>>,
}

impl StateGraph {
    pub fn new(
        initial_net: Graph<PetriNetNode, PetriNetEdge>,
        initial_mark: HashSet<(NodeIndex, usize)>,
    ) -> Self {
        Self {
            graph: Graph::<StateNode, StateEdge>::new(),
            initial_net: Box::new(initial_net),
            initial_mark,
            deadlock_marks: HashSet::new(),
        }
    }

    /// 生成 Petri 网从初始状态可达的所有状态
    ///
    /// 该函数使用广度优先搜索和并行处理的方式来探索所有可达状态。
    /// 对于每个状态，计算其使能的变迁，并行地发生这些变迁以生成新状态，
    /// 如果生成的新状态是唯一的，则将其添到状态图中。
    ///
    /// 具体实现：
    /// 1. 使用 rayon 进行变迁的并行处理，充分利用多核 CPU
    /// 2. 使用 Arc<Mutex<>> 保护共享状态，确保线程安全
    /// 3. 动态构建状态图，记录状态间的变迁关系
    /// 4. 使用队列存储待处理的状态，保证广度优先的搜索顺序
    pub fn generate_states(&mut self) {
        let mut queue = VecDeque::new();
        let all_states = Arc::new(Mutex::new(HashSet::<Vec<(usize, usize)>>::new()));
        let mut visited_states = HashSet::new();
        // 初始化状态队列，加入初始网和标识
        queue.push_back((self.initial_net.clone(), self.initial_mark.clone()));
        {
            all_states
                .lock()
                .unwrap()
                .insert(normalize_state(&self.initial_mark));
        }
        while let Some((mut current_net, current_mark)) = queue.pop_front() {
            // 获取当前状态下所有使能的变迁

            let enabled_transitions = self.get_enabled_transitions(&mut current_net, &current_mark);

            // 如果没有使能的变迁，将当前状态添加到死锁标识集合中
            if enabled_transitions.is_empty() {
                let current_state_normalized = normalize_state(&current_mark);
                self.deadlock_marks.insert(current_state_normalized.clone());
                continue;
            }

            let current_state = normalize_state(&current_mark);
            if !visited_states.insert(current_state.clone()) {
                continue; // 跳过已访问的状态
            }
            let current_node = self.graph.add_node(StateNode::new(current_state.clone()));
            // 并行处理每个变迁，生成新状态，同时保存变迁信息
            // let new_states: Vec<_> = enabled_transitions
            //     .into_par_iter()
            //     .map(|transition| {
            //         // 为每个线程创建独立的网络副本
            //         let mut net_clone = current_net.clone();
            //         let (new_net, new_mark) =
            //             self.fire_transition(&mut net_clone, &current_mark, transition);
            //         (transition, new_net, new_mark)
            //     })
            //     .collect();

            // 在 generate_states 方法中替换 rayon 并行处理部分
            let new_states: Vec<_> = {
                let mut handles = vec![];

                for transition in enabled_transitions {
                    let current_net = current_net.clone();
                    let current_mark = current_mark.clone();
                    let self_clone = self.clone();

                    let handle = std::thread::spawn(move || {
                        let mut net_clone = current_net.clone();
                        let (new_net, new_mark) =
                            self_clone.fire_transition(&mut net_clone, &current_mark, transition);
                        (transition, new_net, new_mark)
                    });

                    handles.push(handle);
                }

                // 收集所有线程的结果
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect()
            };

            // 处理每个新生成的状态
            for (transition, new_net, new_mark) in new_states {
                let new_state = normalize_state(&new_mark);
                // std::thread::sleep(std::time::Duration::from_millis(500));
                // 检查新状态是否唯一，如果是则添加到状态图中
                let mut all_states_guard = all_states.lock().unwrap();
                if insert_with_comparison(&mut all_states_guard, &new_state) {
                    // if all_states_guard.insert(new_state.clone()) {
                    // 将新状态加入队列，等待后续处理
                    queue.push_back((new_net.clone(), new_mark.clone()));
                    // log::info!("new state: {:?}", new_state);
                    // 在状态图中添加新状态节点
                    let new_node = self.graph.add_node(StateNode::new(new_state));

                    // 添加从当前状态到新状态的边，边的标签为变迁名
                    self.graph.add_edge(
                        current_node,
                        new_node,
                        StateEdge::new(format!("{:?}", transition), 1),
                    );
                }
            }
        }
    }

    #[inline]
    fn set_current_mark(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) {
        // 首先将所有库所的 token 清零
        for node_index in net.node_indices() {
            if let Some(PetriNetNode::P(place)) = net.node_weight(node_index) {
                *place.tokens.write().unwrap() = 0;
            }
        }

        // 直接根据 mark 中的 NodeIndex 设置对应的 token
        for (node_index, token_count) in mark {
            if let Some(PetriNetNode::P(place)) = net.node_weight(*node_index) {
                // let tokens = *place.tokens.write().unwrap();
                {
                    *place.tokens.write().unwrap() = *token_count;
                }
                assert!(
                    *place.tokens.read().unwrap() <= place.capacity,
                    "Token count ({}) exceeds capacity ({}) at node index {}, and token_count is {} ",
                    *place.tokens.read().unwrap(),
                    place.capacity,
                    node_index.index(),
                    token_count
                );
            }
        }
    }

    /// 获取当前标识下所有使能的变迁
    ///
    /// # 参数
    /// * `net` - 当前 Petri 网的可变引用
    /// * `mark` - 当前标识（状态）
    ///
    /// # 返回值
    /// 返回一个包含所有使能变迁节点索引的向量
    ///
    /// # 处理流程
    /// 1. 使用 `set_current_mark` 函数设置当前标识
    /// 2. 遍历网络中的每个节点，检查其是否为变迁节点
    /// 3. 对于每个变迁节点，检查其所有输入库所是否有足够的 token
    /// 4. 如果所有输入库所的 token 数量均满足要求，则该变迁为使能状态
    /// 5. 将所有使能的变迁节点索引添加到返回的向量中
    fn get_enabled_transitions(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
    ) -> Vec<NodeIndex> {
        let mut sched_transiton = Vec::<NodeIndex>::new();

        // 使用内联函数设置当前标识
        self.set_current_mark(net, mark);

        // 检查变迁使能的逻辑
        for node_index in net.node_indices() {
            match net.node_weight(node_index) {
                Some(PetriNetNode::T(_)) => {
                    let mut enabled = true;
                    for edge in net.edges_directed(node_index, Direction::Incoming) {
                        match net.node_weight(edge.source()).unwrap() {
                            PetriNetNode::P(place) => {
                                if *place.tokens.read().unwrap() < edge.weight().label {
                                    enabled = false;
                                    break;
                                }
                            }
                            _ => {
                                log::error!("The predecessor set of transition is not place");
                            }
                        }
                    }
                    if enabled {
                        sched_transiton.push(node_index);
                    }
                }
                _ => continue,
            }
        }

        sched_transiton
    }

    /// 发生一个变迁并生成新的网络状态
    ///
    /// # 参数
    /// * `net` - 当前 Petri 网的可变引用
    /// * `mark` - 当前标识（状态）
    /// * `transition` - 要发生的变迁节点索引
    ///
    /// # 返回值
    /// 返回一个元组，包含：
    /// * 发生变迁后的新网络
    /// * 新的标识（状态）
    ///
    /// # 处理流程
    /// 1. 克隆当前网络创建新图
    /// 2. 根据当前标识设置初始 token
    /// 3. 从变迁的输入库所中减去相应的 token
    /// 4. 向变迁的输出库所中添加相应的 token（考虑容量限制）
    /// 5. 生成并返回新的状态
    fn fire_transition(
        &self,
        net: &mut Graph<PetriNetNode, PetriNetEdge>,
        mark: &HashSet<(NodeIndex, usize)>,
        transition: NodeIndex,
    ) -> (
        Box<Graph<PetriNetNode, PetriNetEdge>>,
        HashSet<(NodeIndex, usize)>,
    ) {
        let mut new_net = net.clone(); // 克隆当前网，创建新图
        self.set_current_mark(&mut new_net, mark);
        let mut new_state = HashSet::<(NodeIndex, usize)>::new();
        log::debug!("The transition to fire is: {}", transition.index());

        // 从输入库所中减去token
        log::debug!("sub token to source node!");
        for edge in new_net.edges_directed(transition, Direction::Incoming) {
            match new_net.node_weight(edge.source()).unwrap() {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens -= edge.weight().label;
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        // 将token添加到输出库所中
        log::debug!("add token to target node!");
        for edge in new_net.edges_directed(transition, Direction::Outgoing) {
            let place_node = new_net.node_weight(edge.target()).unwrap();
            match place_node {
                PetriNetNode::P(place) => {
                    let mut tokens = place.tokens.write().unwrap();
                    *tokens += edge.weight().label;
                    if *tokens > place.capacity {
                        *tokens = place.capacity;
                    }
                    assert!(place.capacity > 0);
                }
                PetriNetNode::T(_) => {
                    log::error!("{}", "this error!");
                }
            }
        }

        log::debug!("generate new state!");
        for node in new_net.node_indices() {
            match &new_net[node] {
                PetriNetNode::P(place) => {
                    let tokens = *place.tokens.read().unwrap();
                    if tokens > 0 {
                        // 确保token数量不超过容量限制
                        let final_tokens = tokens.min(place.capacity);
                        new_state.insert((node, final_tokens));
                    }
                }
                PetriNetNode::T(_) => {}
            }
        }

        (Box::new(new_net), new_state) // 返回新图和新状态
    }

    // Check Deadlock
    pub fn check_deadlock(&mut self) -> String {
        use petgraph::graph::node_index;
        // Remove the terminal mark
        self.deadlock_marks.retain(|v| {
            v.iter().all(|m| match &self.initial_net[node_index(m.0)] {
                PetriNetNode::P(p) => !p.name.contains("mainend"),
                _ => false,
            })
        });

        if self.deadlock_marks.is_empty() {
            return "No deadlock detected.\n".to_string();
        }

        let mut result = String::from("Detected deadlock states:\n");
        for (i, mark) in self.deadlock_marks.iter().enumerate() {
            result.push_str(&format!("\nDeadlock State #{}\n", i + 1));
            result.push_str("Active Places:\n");

            let places: Vec<String> = mark
                .iter()
                .filter_map(|x| match &self.initial_net[node_index(x.0)] {
                    PetriNetNode::P(p) => Some(format!(
                        "  - {} (tokens: {}, location: {})",
                        p.name, x.1, p.span
                    )),
                    _ => None,
                })
                .collect();

            result.push_str(&places.join("\n"));
            result.push('\n');
        }

        result
    }

    /// 将状态图以 DOT 格式输出
    ///
    /// # 功能
    /// * 直接打印到标准输出: 不带参数调用
    /// * 输出到文件: 提供文件路径参数
    ///
    /// # 参数
    /// * `path` - 可选的输出文件路径
    ///
    /// # 返回值
    /// * `Result<(), std::io::Error>` - 写入文件时可能产生错误
    ///
    /// # 示例
    /// ```no_run
    /// // 打印到标准输出
    /// state_graph.dot(None);
    ///
    /// // 输出到文件
    /// state_graph.dot(Some("output.dot"))?;
    /// ```
    #[allow(dead_code)]
    pub fn dot(&self, path: Option<&str>) -> std::io::Result<()> {
        let dot_string = format!(
            "digraph {{\n{:?}\n}}",
            Dot::with_config(&self.graph, &[Config::GraphContentOnly])
        );

        match path {
            Some(file_path) => {
                use std::fs::File;
                use std::io::Write;
                let mut file = File::create(file_path)?;
                file.write_all(dot_string.as_bytes())?;
                Ok(())
            }
            None => {
                println!("{}", dot_string);
                Ok(())
            }
        }
    }
}
