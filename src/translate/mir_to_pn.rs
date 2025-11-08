use crate::net::core::Net;
use crate::net::structure::{Place, PlaceType, Transition, TransitionType};
use crate::net::PlaceId;
use crate::util::format_name;

use rustc_middle::mir::{BasicBlock, Body, TerminatorKind};
use rustc_middle::ty::{Instance, TyCtxt};
use std::collections::HashMap;

pub struct BodyToPetriNet<'translate, 'tcx> {
    instance: &'translate Instance<'tcx>,
    body: &'translate Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    net: &'translate mut Net,
    entry_exit: (PlaceId, PlaceId),
    bb_places: HashMap<BasicBlock, PlaceId>,
    function_name: String,
}

impl<'translate, 'tcx> BodyToPetriNet<'translate, 'tcx> {
    pub fn new(
        instance: &'translate Instance<'tcx>,
        body: &'translate Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        net: &'translate mut Net,
        entry_exit: (PlaceId, PlaceId),
    ) -> Self {
        let function_name = format_name(instance.def_id());
        Self {
            instance,
            body,
            tcx,
            net,
            entry_exit,
            bb_places: HashMap::new(),
            function_name,
        }
    }

    pub fn translate(&mut self) {
        self.initialize_basic_blocks();
        self.connect_entry_block();
        self.translate_control_flow();
    }

    fn initialize_basic_blocks(&mut self) {
        for (bb_idx, bb) in self.body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                continue;
            }
            let span = format!("{:?}", bb.terminator().source_info.span);
            let place = Place::new(
                format!("{}_bb{}", self.function_name, bb_idx.index()),
                0,
                u64::MAX,
                PlaceType::BasicBlock,
                span,
            );
            let place_id = self.net.add_place(place);
            self.bb_places.insert(bb_idx, place_id);
        }
    }

    fn connect_entry_block(&mut self) {
        let entry_block = BasicBlock::from_usize(0);
        let Some(entry_place) = self.bb_places.get(&entry_block).copied() else {
            return;
        };
        let transition = Transition::new_with_transition_type(
            format!("{}_entry", self.function_name),
            TransitionType::Start(0),
        );
        let transition_id = self.net.add_transition(transition);
        self.net.add_input_arc(self.entry_exit.0, transition_id, 1);
        self.net.add_output_arc(entry_place, transition_id, 1);
    }

    fn translate_control_flow(&mut self) {
        for (bb_idx, bb) in self.body.basic_blocks.iter_enumerated() {
            if bb.is_cleanup || bb.is_empty_unreachable() {
                continue;
            }
            let terminator = bb.terminator();
            match &terminator.kind {
                TerminatorKind::Goto { target } => {
                    self.link_blocks(bb_idx, *target, TransitionType::Goto, "goto");
                }
                TerminatorKind::SwitchInt { targets, .. } => {
                    for (idx, target) in targets.all_targets().iter().enumerate() {
                        self.link_blocks(
                            bb_idx,
                            *target,
                            TransitionType::Switch,
                            &format!("switch_{idx}"),
                        );
                    }
                }
                TerminatorKind::Call { target, .. } => {
                    if let Some(target) = target {
                        self.link_blocks(bb_idx, *target, TransitionType::Goto, "call");
                    } else {
                        self.link_to_exit(bb_idx, TransitionType::Return(0), "call_exit");
                    }
                }
                TerminatorKind::Return => {
                    self.link_to_exit(bb_idx, TransitionType::Return(0), "return");
                }
                _ => {
                    self.link_to_exit(bb_idx, TransitionType::Normal, "fallback");
                }
            }
        }
    }

    fn link_blocks(&mut self, from: BasicBlock, to: BasicBlock, ty: TransitionType, label: &str) {
        let Some(source_place) = self.bb_places.get(&from).copied() else {
            return;
        };
        let Some(target_place) = self.bb_places.get(&to).copied() else {
            return;
        };
        let transition = Transition::new_with_transition_type(
            format!("{}_{}_{}", self.function_name, from.index(), label),
            ty,
        );
        let transition_id = self.net.add_transition(transition);
        self.net.add_input_arc(source_place, transition_id, 1);
        self.net.add_output_arc(target_place, transition_id, 1);
    }

    fn link_to_exit(&mut self, from: BasicBlock, ty: TransitionType, label: &str) {
        let Some(source_place) = self.bb_places.get(&from).copied() else {
            return;
        };
        let transition = Transition::new_with_transition_type(
            format!("{}_{}_{}", self.function_name, from.index(), label),
            ty,
        );
        let transition_id = self.net.add_transition(transition);
        self.net.add_input_arc(source_place, transition_id, 1);
        self.net.add_output_arc(self.entry_exit.1, transition_id, 1);
    }
}
