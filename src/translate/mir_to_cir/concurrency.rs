//! 原子与 channel 辅助（与 `mir_to_pn/concurrency` 重复，仅发射 CIR）

use super::BodyToCir;
use crate::{
    concurrency::atomic::AtomicOrdering,
    memory::pointsto::AliasId,
    net::{PlaceId, TransitionType},
};
use rustc_middle::mir::BasicBlock;

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    pub(super) fn find_atomic_matches(&mut self, current_id: &AliasId) -> Vec<(AliasId, PlaceId)> {
        let mut matches = Vec::new();
        for (alias_id, place_ids) in self.resources.atomic_places().iter() {
            if self
                .alias
                .borrow_mut()
                .alias_atomic(*current_id, *alias_id)
                .may_alias(self.alias_unknown_policy)
            {
                for &place_id in place_ids {
                    matches.push((*alias_id, place_id));
                }
            }
        }
        matches
    }

    #[cfg(not(feature = "atomic-violation"))]
    pub(super) fn handle_atomic_basic_op<F>(
        &mut self,
        _op_name: &str,
        current_id: AliasId,
        bb_idx: &BasicBlock,
        span: &str,
        mut transition_builder: F,
    ) -> bool
    where
        F: FnMut(&AliasId, &AtomicOrdering, String) -> TransitionType,
    {
        let matches = self.find_atomic_matches(&current_id);
        if matches.is_empty() {
            return false;
        }
        let span_owned = span.to_string();
        if let Some(order) = self.resources.atomic_orders().get(&current_id) {
            for (_idx, (alias_id, _resource_place)) in matches.into_iter().enumerate() {
                let transition_type = transition_builder(&alias_id, order, span_owned.clone());
                self.emit_tt(&transition_type, *bb_idx, span);
            }
        }
        true
    }

    #[cfg(feature = "atomic-violation")]
    pub(super) fn handle_atomic_basic_op<F>(
        &mut self,
        op_name: &str,
        current_id: AliasId,
        bb_idx: &BasicBlock,
        span: &str,
        _transition_builder: F,
    ) -> bool
    where
        F: FnMut(&AliasId, &AtomicOrdering, String) -> TransitionType,
    {
        let matches = self.find_atomic_matches(&current_id);
        if matches.is_empty() {
            log::warn!("no alias found for atomic operation in {:?}", span);
            return false;
        }
        let Some(order) = self.resources.atomic_orders().get(&current_id).copied() else {
            log::warn!(
                "[atomic-violation] missing ordering for {} @ {:?}",
                op_name,
                span
            );
            return true;
        };
        let tid = self.instance_id.index();
        let span_owned = span.to_string();
        let alias_id = matches.first().map(|(a, _)| *a).unwrap_or(current_id);
        let tt = match op_name {
            "load" => TransitionType::AtomicLoad(alias_id, order, span_owned.clone(), tid),
            "store" => TransitionType::AtomicStore(alias_id, order, span_owned.clone(), tid),
            _ => return false,
        };
        self.emit_tt(&tt, *bb_idx, span);
        true
    }

    pub(super) fn find_channel_place(&mut self, channel_alias: AliasId) -> Option<PlaceId> {
        for (alias_id, node) in self.resources.channel_places().iter() {
            let alias_kind = self
                .alias
                .borrow_mut()
                .alias_atomic(channel_alias, *alias_id);
            if alias_kind.may_alias(self.alias_unknown_policy) {
                return Some(*node);
            }
        }
        None
    }
}
