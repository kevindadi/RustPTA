use super::petri_net::PetriNet;

pub trait OutputType {
    fn lola(&self);

    fn pnml(&self);
}

impl<'tcx, 'a> OutputType for PetriNet<'tcx, 'a> {
    fn lola(&self) {}

    fn pnml(&self) {}
}
