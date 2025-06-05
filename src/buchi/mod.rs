use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::{
    ltl::automata::{Node, INIT_NODE_ID},
    ltl::expression::LTLExpression,
};

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct BuchiNode {
    pub id: String,
    pub labels: Vec<LTLExpression>,
    pub adj: Vec<BuchiNode>,
}

impl BuchiNode {
    pub fn new(id: String) -> Self {
        Self {
            id,
            labels: Vec::new(),
            adj: Vec::new(),
        }
    }
}

impl fmt::Display for BuchiNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buff = String::new();
        buff.push_str(&format!("{}id = {}\n", &buff, self.id));

        let labels = self
            .labels
            .iter()
            .fold("".to_string(), |acc, label| acc + &format!("{},", label));
        buff.push_str(&format!("{}{}.labels = [{}]\n", &buff, self.id, labels));

        let adjs = self
            .adj
            .iter()
            .fold("".to_string(), |acc, a| acc + &format!("{},", a.id));
        buff.push_str(&format!("{}{}.adj = [{}]\n", &buff, self.id, adjs));

        write!(f, "{}", buff)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct GeneralBuchi {
    pub states: Vec<String>,
    pub accepting_states: Vec<Vec<BuchiNode>>,
    pub init_states: Vec<BuchiNode>,
    pub adj_list: Vec<BuchiNode>,
}

impl fmt::Display for GeneralBuchi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buff = String::new();
        for (i, ac) in self.accepting_states.iter().enumerate() {
            let states = ac
                .iter()
                .fold("".to_string(), |acc, a| acc + &format!("{},", a.id));
            buff.push_str(&format!("{}accepting_state[{}] = {:?}\n", &buff, i, states));
        }

        let init_states = self
            .init_states
            .iter()
            .fold("".to_string(), |acc, init| acc + &format!("{},", init.id));
        buff.push_str(&format!("{}init_states = [{}]\n", &buff, init_states));

        let adjs = self
            .adj_list
            .iter()
            .fold("".to_string(), |acc, adj| acc + &format!("{},", adj.id));
        buff.push_str(&format!("{}adj = [{}]\n", &buff, adjs));

        write!(f, "{}", buff)
    }
}

impl GeneralBuchi {
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            accepting_states: Vec::new(),
            init_states: Vec::new(),
            adj_list: Vec::new(),
        }
    }

    pub fn get_node(&self, name: &str) -> Option<BuchiNode> {
        for adj in self.adj_list.iter() {
            if adj.id == name {
                return Some(adj.clone());
            }
        }

        None
    }

    pub fn get_node_mut(&mut self, name: &str) -> Option<&mut BuchiNode> {
        for adj in self.adj_list.iter_mut() {
            if adj.id == name {
                return Some(adj);
            }
        }

        None
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Buchi {
    pub states: Vec<String>,
    pub accepting_states: Vec<BuchiNode>,
    pub init_states: Vec<BuchiNode>,
    pub adj_list: Vec<BuchiNode>,
}

impl Buchi {
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            accepting_states: Vec::new(),
            init_states: Vec::new(),
            adj_list: Vec::new(),
        }
    }

    pub fn get_node(&self, name: &str) -> Option<BuchiNode> {
        for adj in self.adj_list.iter() {
            if adj.id == name {
                return Some(adj.clone());
            }
        }

        None
    }

    pub fn get_node_mut(&mut self, name: &str) -> Option<&mut BuchiNode> {
        for adj in self.adj_list.iter_mut() {
            if adj.id == name {
                return Some(adj);
            }
        }

        None
    }
}

fn extract_unitl_subf(
    f: &LTLExpression,
    mut sub_formulas: Vec<LTLExpression>,
) -> Vec<LTLExpression> {
    match f {
        LTLExpression::True => sub_formulas,
        LTLExpression::False => sub_formulas,
        LTLExpression::Literal(_) => sub_formulas,
        LTLExpression::Not(_) => sub_formulas,
        LTLExpression::And(f1, f2) => extract_unitl_subf(f2, extract_unitl_subf(f1, sub_formulas)),
        LTLExpression::Or(f1, f2) => extract_unitl_subf(f2, extract_unitl_subf(f1, sub_formulas)),
        LTLExpression::U(f1, f2) => {
            sub_formulas.push(LTLExpression::U(f1.clone(), f2.clone()));
            extract_unitl_subf(f2, extract_unitl_subf(f1, sub_formulas))
        }
        LTLExpression::R(f1, f2) => extract_unitl_subf(f1, extract_unitl_subf(f2, sub_formulas)),
        LTLExpression::V(f1, f2) => extract_unitl_subf(f1, extract_unitl_subf(f2, sub_formulas)),
        e => panic!(
            "unsuported operator, you should simplify the expression: {}",
            e
        ),
    }
}

pub fn extract_buchi(result: Vec<Node>, f: LTLExpression) -> GeneralBuchi {
    let mut buchi = GeneralBuchi::new();

    for n in result.iter() {
        let mut buchi_node = BuchiNode::new(n.id.clone());
        buchi.states.push(n.id.clone());

        for l in n.oldf.iter() {
            match *l {
                LTLExpression::Literal(ref lit) => {
                    buchi_node.labels.push(LTLExpression::Literal(lit.clone()))
                }
                LTLExpression::True => buchi_node.labels.push(LTLExpression::True),
                LTLExpression::False => buchi_node.labels.push(LTLExpression::False),
                LTLExpression::Not(ref e) => match **e {
                    LTLExpression::True => buchi_node.labels.push(LTLExpression::False),
                    LTLExpression::False => buchi_node.labels.push(LTLExpression::True),
                    LTLExpression::Literal(ref lit) => buchi_node.labels.push(LTLExpression::Not(
                        Box::new(LTLExpression::Literal(lit.into())),
                    )),
                    _ => {}
                },
                _ => {}
            }
        }
        buchi.adj_list.push(buchi_node);
    }

    let mut initial_states = Vec::new();

    for n in result.iter() {
        let buchi_node = buchi.get_node(&n.id).unwrap();

        for k in n.incoming.iter() {
            if k.id == INIT_NODE_ID.to_string() {
                initial_states.push(buchi_node.clone());
            } else {
                buchi
                    .get_node_mut(&k.id)
                    .unwrap()
                    .adj
                    .push(buchi_node.clone());
            }
        }
    }

    let mut init_state = BuchiNode::new(INIT_NODE_ID.to_string());
    buchi.states.push(INIT_NODE_ID.to_string());
    init_state.adj = initial_states.clone();
    buchi.adj_list.push(init_state);
    buchi.init_states = initial_states;

    let sub_formulas = extract_unitl_subf(&f, vec![]);

    for f in sub_formulas {
        let mut accepting_states = Vec::new();

        for n in result.iter() {
            match f {
                LTLExpression::U(_, ref f2) if !n.oldf.contains(&f) || n.oldf.contains(f2) => {
                    if let Some(node) = buchi.get_node(&n.id) {
                        accepting_states.push(node);
                    }
                }
                _ => {}
            }
        }

        buchi.accepting_states.push(accepting_states);
    }

    buchi
}

impl From<GeneralBuchi> for Buchi {
    fn from(general_buchi: GeneralBuchi) -> Buchi {
        let mut ba = Buchi::new();

        if general_buchi.accepting_states.is_empty() {
            ba.accepting_states = general_buchi.adj_list.clone();
            ba.adj_list = general_buchi.adj_list.clone();
            ba.init_states = general_buchi.init_states.clone();
            return ba;
        }
        for (i, _) in general_buchi.accepting_states.iter().enumerate() {
            for n in general_buchi.adj_list.iter() {
                let mut buchi_node = BuchiNode::new(format!("{}{}", n.id, i));
                buchi_node.labels = n.labels.clone();
                ba.adj_list.push(buchi_node);
            }
        }
        for (i, f) in general_buchi.accepting_states.iter().enumerate() {
            for node in general_buchi.adj_list.iter() {
                for adj in node.adj.iter() {
                    let j;
                    if f.iter().any(|n| n.id == node.id) {
                        j = (i + 1) % general_buchi.accepting_states.len();
                    } else {
                        j = i;
                    }
                    let ba_node = ba
                        .get_node_mut(format!("{}{}", node.id, i).as_str())
                        .unwrap();
                    ba_node.adj.push(BuchiNode {
                        id: format!("{}{}", adj.id, j),
                        labels: adj.labels.clone(),
                        adj: vec![],
                    });
                }
            }
        }

        let init_node = ba
            .get_node(format!("{}0", INIT_NODE_ID).as_str())
            .expect(&format!(
                "cannot find the init node {}0 but it should exist",
                INIT_NODE_ID
            ));
        ba.init_states.push(init_node.clone());

        let f_1 = general_buchi.accepting_states.first().unwrap();
        for accepting_state in f_1.iter() {
            let node = ba
                .get_node(format!("{}0", accepting_state.id).as_str())
                .unwrap();
            ba.accepting_states.push(node);
        }
        ba
    }
}

pub fn product_automata(program: Buchi, property: Buchi) -> Buchi {
    let mut product_buchi = Buchi::new();

    for n1 in program.adj_list.iter() {
        for n2 in property.adj_list.iter() {
            let product_id = format!("{}_{}", n1.id, n2.id);
            let product_node = BuchiNode::new(product_id);
            product_buchi.adj_list.push(product_node);
        }
    }

    for bn1 in product_buchi.adj_list.clone().iter() {
        let names: Vec<&str> = bn1.id.split('_').collect();
        let q1 = program.get_node(names[0]).unwrap();
        let q1_prime = property.get_node(names[1]).unwrap();

        for bn2 in product_buchi.adj_list.clone().iter() {
            let names: Vec<&str> = bn2.id.split('_').collect();
            let q2 = program.get_node(names[0]).unwrap();
            let q2_prime = property.get_node(names[1]).unwrap();

            let mut labels = HashSet::new();
            labels.extend(q1_prime.labels.iter());
            labels.extend(q2_prime.labels.iter());

            for label in labels {
                if q1
                    .adj
                    .iter()
                    .any(|b| b.id == q2.id && b.labels.contains(&label))
                    && q1_prime
                        .adj
                        .iter()
                        .any(|b| b.id == q2_prime.id && b.labels.contains(&label))
                {
                    if let Some(product_node) = product_buchi.get_node_mut(bn1.id.as_str()) {
                        let mut tmp_node = bn2.clone();
                        tmp_node.labels = vec![label.clone()];
                        (*product_node).adj.push(tmp_node.clone());
                    }
                }
            }
        }
    }

    for a in program.accepting_states.iter() {
        for adj in product_buchi.adj_list.iter() {
            let names: Vec<&str> = adj.id.split('_').collect();
            if a.id == names[0] {
                product_buchi.accepting_states.push(adj.clone());
            }
        }
    }

    for a in property.accepting_states.iter() {
        for adj in product_buchi.adj_list.iter() {
            let names: Vec<&str> = adj.id.split('_').collect();
            if a.id == names[1] {
                product_buchi.accepting_states.push(adj.clone());
            }
        }
    }

    if let Some(node) = product_buchi.get_node("INIT_INIT0") {
        product_buchi.init_states = vec![node];
    } else if let Some(node) = product_buchi.get_node("INIT_INIT") {
        product_buchi.init_states = vec![node];
    } else {
        unreachable!("cannot find the INIT product state, this should not happend");
    }

    product_buchi
}
