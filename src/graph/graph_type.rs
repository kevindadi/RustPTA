use super::petri_net::PetriNet;

pub trait GraphType {
    fn lola(&self);

    fn pnml(&self);
}

impl<'tcx, 'a> GraphType for PetriNet<'tcx, 'a> {
    fn lola(&self) {}

    fn pnml(&self) {}
}
