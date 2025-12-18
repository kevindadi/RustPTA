use crate::net::structure::{PlaceType, TransitionType};

use super::ReductionStep;
use super::graph::{GraphTransition, ReductionGraph};

impl ReductionGraph {
    /// # 约简规则:中间库所消除(Intermediate Place Elimination)
    ///
    /// 形式化定义
    /// - 在 Petri 网约简图 `G = (P, T, F)` 中选取库所 `p`,满足:
    ///   - `p` 未被移除、类型不是 `Resources`,且令 `tokens(p) = 0`.
    ///   - 存在唯一输入变迁 `t_in` 与唯一输出变迁 `t_out`,它们均未被移除.
    ///   - `|t_in•| = |•t_out| = 1`,并且弧权重一致,即 `w(t_in, p) = w(p, t_out)`.
    /// - 构造新变迁 `t_new`,其输入集合为 `•t_in`,输出集合为 `t_out•`,
    ///   原始标识为二者并集,类型在 `t_in` 与 `t_out` 类型一致时继承,否则退化为 `Normal`.
    /// - 将 `t_in`、`t_out` 与库所 `p` 标记移除,插入 `t_new` 并更新涉入库所的邻接关系.
    pub(crate) fn eliminate_intermediate_places(&mut self) -> Vec<ReductionStep> {
        let mut steps = Vec::new();
        let mut changed = true;

        while changed {
            changed = false;
            for place_idx in 0..self.places.len() {
                if self.places[place_idx].removed {
                    continue;
                }
                if self.places[place_idx].place.place_type == PlaceType::Resources {
                    continue;
                }
                if self.places[place_idx].place.tokens != 0 {
                    continue;
                }

                if self.places[place_idx].incoming.len() != 1
                    || self.places[place_idx].outgoing.len() != 1
                {
                    continue;
                }

                let in_transition_idx = self.places[place_idx].incoming[0];
                let out_transition_idx = self.places[place_idx].outgoing[0];

                if self.transitions[in_transition_idx].removed
                    || self.transitions[out_transition_idx].removed
                {
                    continue;
                }

                if self.transitions[in_transition_idx].outputs.len() != 1 {
                    continue;
                }
                if self.transitions[out_transition_idx].inputs.len() != 1 {
                    continue;
                }

                let weight_in = self.transitions[in_transition_idx].outputs[0].1;
                let weight_out = self.transitions[out_transition_idx].inputs[0].1;
                if weight_in != weight_out {
                    continue;
                }

                let inputs = self.transitions[in_transition_idx].inputs.clone();
                let outputs = self.transitions[out_transition_idx].outputs.clone();
                let place_originals = self.places[place_idx].originals.clone();
                let combined_originals = {
                    let mut data = self.transitions[in_transition_idx].originals.clone();
                    data.extend(self.transitions[out_transition_idx].originals.clone());
                    data
                };

                let new_transition_type = if self.transitions[in_transition_idx]
                    .transition
                    .transition_type
                    == self.transitions[out_transition_idx]
                        .transition
                        .transition_type
                {
                    self.transitions[in_transition_idx]
                        .transition
                        .transition_type
                        .clone()
                } else {
                    TransitionType::Normal
                };

                let new_transition_idx = self.add_transition(GraphTransition::new_with_type(
                    format!("inter_merge#{}", self.merge_counter),
                    new_transition_type,
                    combined_originals.clone(),
                    inputs.clone(),
                    outputs.clone(),
                ));
                self.merge_counter += 1;

                for (p_idx, _) in &inputs {
                    self.places[*p_idx]
                        .outgoing
                        .retain(|idx| *idx != in_transition_idx);
                    self.places[*p_idx].outgoing.push(new_transition_idx);
                }
                for (p_idx, _) in &outputs {
                    self.places[*p_idx]
                        .incoming
                        .retain(|idx| *idx != out_transition_idx);
                    self.places[*p_idx].incoming.push(new_transition_idx);
                }

                self.remove_transition(in_transition_idx);
                self.remove_transition(out_transition_idx);
                self.remove_place(place_idx);
                self.clean_adjacency();

                steps.push(ReductionStep::IntermediatePlaceEliminated {
                    places: place_originals,
                    merged_transitions: combined_originals,
                });

                changed = true;
                break;
            }
        }

        steps
    }
}
