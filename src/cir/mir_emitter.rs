//! Emit CIR statements **during** MIR translation (same order as `BodyToPetriNet`), not from `Net` topology.
use std::collections::BTreeSet;

use crate::cir::naming::abbreviate_function_name;
use crate::cir::op_mapping::transition_to_cir_op;
use crate::cir::resource_table::ResourceTable;
use crate::cir::types::{CirFunction, CirOp, CirStatement, CirTransfer, FunctionKind};
use crate::net::structure::TransitionType;

/// Per-function emitter; borrows the global [`ResourceTable`] shared across the crate's CIR build.
pub struct CirMirEmitter<'a> {
    table: &'a mut ResourceTable,
    spawn_targets: &'a mut BTreeSet<String>,
    prefix: String,
    sid_counter: u32,
    held_locks: Vec<String>,
    body: Vec<CirStatement>,
}

impl<'a> CirMirEmitter<'a> {
    pub fn new(
        func_name: &str,
        table: &'a mut ResourceTable,
        spawn_targets: &'a mut BTreeSet<String>,
    ) -> Self {
        Self {
            table,
            spawn_targets,
            prefix: abbreviate_function_name(func_name),
            sid_counter: 0,
            held_locks: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Record a transition **label** produced by the same MIR path that builds the Petri net.
    pub fn emit(&mut self, tt: &TransitionType, bb_index: usize, span: Option<String>) {
        self.table.ingest(tt);
        let Some(op) = transition_to_cir_op(tt, self.table, &mut self.held_locks) else {
            return;
        };
        if let CirOp::Spawn { spawn } = &op {
            self.spawn_targets.insert(spawn.clone());
        }
        self.sid_counter += 1;
        let sid = format!("{}_{}", self.prefix, self.sid_counter);
        self.body.push(CirStatement {
            sid,
            op: Some(op),
            transfer: CirTransfer::Next {
                next: String::new(),
            },
            span,
            bb_index: Some(bb_index),
        });
    }

    /// Emit a direct call (MIR `Call` terminator) when the callee is in analysis scope.
    pub fn emit_call(&mut self, callee_display: &str, bb_index: usize) {
        self.sid_counter += 1;
        let sid = format!("{}_{}", self.prefix, self.sid_counter);
        self.body.push(CirStatement {
            sid,
            op: Some(CirOp::Call {
                call: callee_display.to_string(),
            }),
            transfer: CirTransfer::Next {
                next: String::new(),
            },
            span: None,
            bb_index: Some(bb_index),
        });
    }

    pub fn finish(mut self, kind: FunctionKind) -> CirFunction {
        let ret_sid = "ret".to_string();
        for i in 0..self.body.len() {
            let next = if i + 1 < self.body.len() {
                self.body[i + 1].sid.clone()
            } else {
                ret_sid.clone()
            };
            self.body[i].transfer = CirTransfer::Next { next };
        }
        self.body.push(CirStatement {
            sid: ret_sid,
            op: None,
            transfer: CirTransfer::done(),
            span: None,
            bb_index: None,
        });
        CirFunction {
            kind,
            body: self.body,
        }
    }
}
