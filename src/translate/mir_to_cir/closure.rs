//! 闭包解析（与 `mir_to_pn/closure` 重复）

use super::BodyToCir;
use crate::net::PlaceId;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{Const, Operand};
use rustc_span::source_map::Spanned;

impl<'translate, 'analysis, 'tcx, 'a> BodyToCir<'translate, 'analysis, 'tcx, 'a> {
    pub(crate) fn resolve_closure_def_id(&self, arg: &Operand<'tcx>) -> Option<DefId> {
        match arg {
            Operand::Move(place) | Operand::Copy(place) => {
                let place_ty = place.ty(self.body, self.tcx).ty;
                match place_ty.kind() {
                    rustc_middle::ty::TyKind::Closure(def_id, _)
                    | rustc_middle::ty::TyKind::FnDef(def_id, _) => Some(*def_id),
                    _ => None,
                }
            }
            Operand::Constant(constant) => {
                let const_val = constant.const_;
                match const_val {
                    Const::Unevaluated(unevaluated, _) => Some(unevaluated.def),
                    _ => {
                        if let rustc_middle::ty::TyKind::Closure(def_id, _)
                        | rustc_middle::ty::TyKind::FnDef(def_id, _) = constant.ty().kind()
                        {
                            Some(*def_id)
                        } else {
                            None
                        }
                    }
                }
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    pub(crate) fn resolve_closure_places(
        &self,
        args: &[Spanned<Operand<'tcx>>],
    ) -> Option<(PlaceId, PlaceId)> {
        args.first()
            .and_then(|arg| self.resolve_closure_def_id(&arg.node))
            .and_then(|def_id| self.functions_map().get(&def_id).copied())
    }

    #[allow(dead_code)] // 与 mir_to_pn/closure 对齐，供后续扩展
    pub(crate) fn resolve_closure_places_at(
        &self,
        args: &[Spanned<Operand<'tcx>>],
        index: usize,
    ) -> Option<(PlaceId, PlaceId)> {
        args.get(index)
            .and_then(|arg| self.resolve_closure_def_id(&arg.node))
            .and_then(|def_id| self.functions_map().get(&def_id).copied())
    }
}
