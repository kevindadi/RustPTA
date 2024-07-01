extern crate rustc_data_structures;
extern crate rustc_index;
extern crate rustc_middle;

use std::collections::VecDeque;

use rustc_data_structures::fx::FxHashSet;
use rustc_index::{Idx, IndexVec};
use rustc_middle::mir::{BasicBlock, Location};
