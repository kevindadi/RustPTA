#[derive(Debug, PartialEq, Eq)]
pub enum AsyncGraphNode<'tcx> {
    Fn(Instance<'tcx>),
    ThreadClosure(Instance<'tcx>),
    AsyncClosure(Instance<'tcx>),
    WithoutBody(Instance<'tcx>),
}

impl AsyncGraphNode {}
