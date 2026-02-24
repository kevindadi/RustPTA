//! Drop 与 unsafe 读写处理：handle_drop、process_rvalue_reads、process_place_writes

use super::BodyToPetriNet;
use crate::{
    concurrency::blocking::{LockGuardId, LockGuardTy},
    memory::pointsto::{AliasId, ApproximateAliasKind},
    net::{Idx, PlaceId, Transition, TransitionType},
};
use rustc_middle::mir::{BasicBlock, BasicBlockData, Operand, Rvalue};

impl<'translate, 'analysis, 'tcx> BodyToPetriNet<'translate, 'analysis, 'tcx> {
    pub(super) fn handle_drop(
        &mut self,
        bb_idx: &BasicBlock,
        place: &rustc_middle::mir::Place<'tcx>,
        target: &BasicBlock,
        name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        let bb_term_name = crate::transition_name!(name, bb_idx, "drop");
        let bb_term_transition =
            Transition::new_with_transition_type(bb_term_name, TransitionType::Drop);
        let bb_end = self.net.add_transition(bb_term_transition);

        self.net
            .add_input_arc(self.bb_graph.last(*bb_idx), bb_end, 1);

        if cfg!(feature = "atomic-violation") {
            self.net
                .add_output_arc(self.bb_graph.start(*target), bb_end, 1);
            return;
        }

        if !bb.is_cleanup {
            let lockguard_id = LockGuardId::new(self.instance_id, place.local);

            if self.lockguards.get(&lockguard_id).is_some() {
                let lock_alias = lockguard_id.get_alias_id();
                let lock_node = self.resources.locks().get(&lock_alias).unwrap();
                match &self.lockguards[&lockguard_id].lockguard_ty {
                    LockGuardTy::StdMutex(_)
                    | LockGuardTy::ParkingLotMutex(_)
                    | LockGuardTy::SpinMutex(_)
                    | LockGuardTy::StdRwLockRead(_)
                    | LockGuardTy::ParkingLotRead(_)
                    | LockGuardTy::SpinRead(_) => {
                        self.net.add_output_arc(*lock_node, bb_end, 1);
                    }
                    _ => {
                        self.net.add_output_arc(*lock_node, bb_end, 10);
                    }
                }

                if let Some(transition) = self.net.get_transition_mut(bb_end) {
                    transition.transition_type = TransitionType::Unlock(lock_node.index());
                }
            }
        }

        self.net
            .add_output_arc(self.bb_graph.start(*target), bb_end, 1);
    }

    pub(super) fn has_unsafe_alias(&self, place_id: AliasId) -> (bool, PlaceId, Option<AliasId>) {
        for (unsafe_place, node_index) in self.resources.unsafe_places().iter() {
            match self
                .alias
                .borrow_mut()
                .alias_atomic(place_id, *unsafe_place)
            {
                ApproximateAliasKind::Probably | ApproximateAliasKind::Possibly => {
                    return (true, *node_index, Some(*unsafe_place));
                }
                _ => return (false, PlaceId::new(0), None),
            }
        }
        (false, PlaceId::new(0), None)
    }

    pub(super) fn process_rvalue_reads(
        &mut self,
        rvalue: &Rvalue<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let places = match rvalue {
            Rvalue::Use(operand) => match operand {
                Operand::Move(place) | Operand::Copy(place) => vec![place],
                Operand::Constant(_) => vec![],
            },
            Rvalue::BinaryOp(_, box (op1, op2)) => {
                let mut places = Vec::new();
                if let Operand::Move(place) | Operand::Copy(place) = op1 {
                    places.push(place);
                }
                if let Operand::Move(place) | Operand::Copy(place) = op2 {
                    places.push(place);
                }
                places
            }
            Rvalue::Ref(_, _, place) => vec![place],
            Rvalue::Discriminant(place) => vec![place],
            Rvalue::Aggregate(_, operands) => operands
                .iter()
                .filter_map(|op| match op {
                    Operand::Move(place) | Operand::Copy(place) => Some(place),
                    _ => None,
                })
                .collect(),

            _ => vec![],
        };

        for place in places {
            let place_id = AliasId::new(self.instance_id, place.local);
            let place_ty = format!("{:?}", place.ty(self.body, self.tcx));

            let alias_result = self.has_unsafe_alias(place_id);
            if alias_result.0 {
                let transition_name =
                    format!("{}_read_{:?}_in:{}", fn_name, place_id.local, span_str);
                let read_t = Transition::new_with_transition_type(
                    transition_name.clone(),
                    TransitionType::UnsafeRead(
                        alias_result.1.index(),
                        span_str.to_string(),
                        bb_idx.index(),
                        place_ty,
                    ),
                );
                let unsafe_read_t = self.net.add_transition(read_t);

                let last_node = self.bb_graph.last(bb_idx);
                self.net.add_input_arc(last_node, unsafe_read_t, 1);

                let unsafe_place = alias_result.1;
                self.net.add_output_arc(unsafe_place, unsafe_read_t, 1);
                self.net.add_input_arc(unsafe_place, unsafe_read_t, 1);

                let place_name = format!("{}_rready", &transition_name.as_str());
                let temp_place_node = crate::bb_place!(self.net, place_name, span_str.to_string());
                self.net.add_output_arc(temp_place_node, unsafe_read_t, 1);

                self.bb_graph.push(bb_idx, temp_place_node);
            }
        }
    }

    pub(super) fn process_place_writes(
        &mut self,
        place: &rustc_middle::mir::Place<'tcx>,
        fn_name: &str,
        bb_idx: BasicBlock,
        span_str: &str,
    ) {
        let place_id = AliasId::new(self.instance_id, place.local);
        let place_ty = format!("{:?}", place.ty(self.body, self.tcx));

        let alias_result = self.has_unsafe_alias(place_id);
        if alias_result.0 {
            let transition_name = format!("{}_write_{:?}_in:{}", fn_name, place_id.local, span_str);
            let write_t = Transition::new_with_transition_type(
                transition_name.clone(),
                TransitionType::UnsafeWrite(
                    alias_result.1.index(),
                    span_str.to_string(),
                    bb_idx.index(),
                    place_ty,
                ),
            );
            let unsafe_write_t = self.net.add_transition(write_t);

            let last_node = self.bb_graph.last(bb_idx);
            self.net.add_input_arc(last_node, unsafe_write_t, 1);

            let unsafe_place = alias_result.1;
            self.net.add_output_arc(unsafe_place, unsafe_write_t, 1);
            self.net.add_input_arc(unsafe_place, unsafe_write_t, 1);

            let place_name = format!("{}_wready", &transition_name.as_str());
            let temp_place_node = crate::bb_place!(self.net, place_name, span_str.to_string());
            self.net.add_output_arc(temp_place_node, unsafe_write_t, 1);

            self.bb_graph.push(bb_idx, temp_place_node);
        }
    }
}
