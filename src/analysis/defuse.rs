
extern crate rustc_middle;

use std::collections::BTreeSet;

use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{Body, Local, Location};

use rustc_middle::mir::visit::{
    MutatingUseContext, NonMutatingUseContext, NonUseContext, PlaceContext,
};

#[derive(Eq, PartialEq, Clone)]
pub enum DefUse {
    Def,
    Use,
    Drop,
}
pub fn categorize(context: PlaceContext) -> Option<DefUse> {
    match context {


        PlaceContext::MutatingUse(MutatingUseContext::Store) |
        PlaceContext::MutatingUse(MutatingUseContext::Call) |
        PlaceContext::MutatingUse(MutatingUseContext::AsmOutput) |
        PlaceContext::MutatingUse(MutatingUseContext::Yield) |
        PlaceContext::NonUse(NonUseContext::StorageLive) |
        PlaceContext::NonUse(NonUseContext::StorageDead) => Some(DefUse::Def),

        PlaceContext::NonMutatingUse(NonMutatingUseContext::Projection) |
        PlaceContext::MutatingUse(MutatingUseContext::Projection) |
        PlaceContext::MutatingUse(MutatingUseContext::Borrow) |
        PlaceContext::NonMutatingUse(NonMutatingUseContext::SharedBorrow) |
        // PlaceContext::NonMutatingUse(NonMutatingUseContext::FakeBorrow) |   
        //??no variant or associated item named `FakeBorrow` found for enum `NonMutatingUseContext`

        PlaceContext::NonMutatingUse(NonMutatingUseContext::PlaceMention) |
        PlaceContext::NonUse(NonUseContext::AscribeUserTy(_)) |
        PlaceContext::MutatingUse(MutatingUseContext::AddressOf) |
        PlaceContext::NonMutatingUse(NonMutatingUseContext::AddressOf) |
        PlaceContext::NonMutatingUse(NonMutatingUseContext::Inspect) |
        PlaceContext::NonMutatingUse(NonMutatingUseContext::Copy) |
        PlaceContext::NonMutatingUse(NonMutatingUseContext::Move) |
        PlaceContext::MutatingUse(MutatingUseContext::Retag) =>
            Some(DefUse::Use),

        PlaceContext::MutatingUse(MutatingUseContext::Drop) =>
            Some(DefUse::Drop),

        PlaceContext::NonUse(NonUseContext::VarDebugInfo) => None,

        PlaceContext::MutatingUse(MutatingUseContext::Deinit | MutatingUseContext::SetDiscriminant) => None,
        _=>None //
    }
}

pub fn find_uses(body: &Body<'_>, local: Local) -> BTreeSet<Location> {
    let mut visitor = AllLocalUsesVisitor {
        for_local: local,
        uses: BTreeSet::default(),
    };
    visitor.visit_body(body);
    visitor.uses
}

struct AllLocalUsesVisitor {
    for_local: Local,
    uses: BTreeSet<Location>,
}

impl<'tcx> Visitor<'tcx> for AllLocalUsesVisitor {
    fn visit_local(&mut self, local: Local, context: PlaceContext, location: Location) {
        if local == self.for_local {
            if let Some(DefUse::Use) = categorize(context) {
                self.uses.insert(location);
            }
        }
    }
}
