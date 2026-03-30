//! `Drop` 终结符上的锁守卫释放（不处理 unsafe 块内的数据竞争）

use super::BodyToCir;
use crate::{
    concurrency::blocking::LockGuardId,
    net::Idx,
    net::structure::TransitionType,
};
use rustc_middle::mir::{BasicBlock, BasicBlockData};

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    pub(super) fn handle_drop(
        &mut self,
        bb_idx: &BasicBlock,
        place: &rustc_middle::mir::Place<'tcx>,
        _target: &BasicBlock,
        _name: &str,
        bb: &BasicBlockData<'tcx>,
    ) {
        if cfg!(feature = "atomic-violation") {
            return;
        }

        if !bb.is_cleanup {
            let lockguard_id = LockGuardId::new(self.instance_id, place.local);
            if self.lockguards.get(&lockguard_id).is_some() {
                let lock_alias = lockguard_id.get_alias_id();
                let lock_node = self.resources.locks().get(&lock_alias).unwrap();
                self.emit_tt(
                    &TransitionType::Unlock(lock_node.index()),
                    *bb_idx,
                    "",
                );
            }
        }
    }
}
