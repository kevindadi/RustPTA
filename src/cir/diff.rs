//! Compare expected CIR (e.g. from an LLM) with extracted CIR by **op shape** and resource **kind**, not names.
use crate::cir::types::{CirArtifact, CirOp, ResourceKind};

#[derive(Debug, Clone)]
pub struct CirDiff {
    pub resource_diffs: Vec<ResourceDiff>,
    pub function_diffs: Vec<FunctionDiff>,
    pub protection_diffs: Vec<ProtectionDiff>,
}

#[derive(Debug, Clone)]
pub enum ResourceDiff {
    Missing {
        name: String,
        kind: ResourceKind,
    },
    Extra {
        name: String,
        kind: ResourceKind,
    },
    KindMismatch {
        name: String,
        expected: ResourceKind,
        actual: ResourceKind,
    },
    PairingMismatch {
        cv: String,
        expected_mutex: String,
        actual_mutex: String,
    },
}

#[derive(Debug, Clone)]
pub enum FunctionDiff {
    Missing {
        name: String,
    },
    Extra {
        name: String,
    },
    BodyDiff {
        function: String,
        alignment: Vec<AlignmentEntry>,
    },
}

#[derive(Debug, Clone)]
pub enum AlignmentEntry {
    Match {
        expected_sid: String,
        actual_sid: String,
    },
    ExpectedOnly {
        sid: String,
        op: String,
    },
    ActualOnly {
        sid: String,
        op: String,
    },
    OpMismatch {
        expected_sid: String,
        actual_sid: String,
        expected_op: String,
        actual_op: String,
    },
}

#[derive(Debug, Clone)]
pub enum ProtectionDiff {
    Unprotected {
        var: String,
        expected_locks: Vec<String>,
    },
    DifferentLock {
        var: String,
        expected: Vec<String>,
        actual: Vec<String>,
    },
}

impl CirDiff {
    pub fn compare(expected: &CirArtifact, extracted: &CirArtifact) -> Self {
        let mut resource_diffs = Vec::new();
        for (name, r) in &expected.resources {
            match extracted.resources.get(name) {
                None => resource_diffs.push(ResourceDiff::Missing {
                    name: name.clone(),
                    kind: r.kind.clone(),
                }),
                Some(a) if a.kind != r.kind => resource_diffs.push(ResourceDiff::KindMismatch {
                    name: name.clone(),
                    expected: r.kind.clone(),
                    actual: a.kind.clone(),
                }),
                Some(a) => {
                    if let (Some(ep), Some(ap)) = (&r.paired_with, &a.paired_with) {
                        if ep != ap {
                            resource_diffs.push(ResourceDiff::PairingMismatch {
                                cv: name.clone(),
                                expected_mutex: ep.clone(),
                                actual_mutex: ap.clone(),
                            });
                        }
                    }
                }
            }
        }
        for (name, r) in &extracted.resources {
            if !expected.resources.contains_key(name) {
                resource_diffs.push(ResourceDiff::Extra {
                    name: name.clone(),
                    kind: r.kind.clone(),
                });
            }
        }

        let mut function_diffs = Vec::new();
        for (name, f) in &expected.functions {
            if !extracted.functions.contains_key(name) {
                function_diffs.push(FunctionDiff::Missing {
                    name: name.clone(),
                });
            } else {
                let alignment = align_bodies(&f.body, &extracted.functions[name].body);
                let has_issue = alignment.iter().any(|e| {
                    !matches!(e, AlignmentEntry::Match { .. })
                });
                if has_issue {
                    function_diffs.push(FunctionDiff::BodyDiff {
                        function: name.clone(),
                        alignment,
                    });
                }
            }
        }
        for name in extracted.functions.keys() {
            if !expected.functions.contains_key(name) {
                function_diffs.push(FunctionDiff::Extra {
                    name: name.clone(),
                });
            }
        }

        let mut protection_diffs = Vec::new();
        for (var, locks) in &expected.protection {
            match extracted.protection.get(var) {
                None => protection_diffs.push(ProtectionDiff::Unprotected {
                    var: var.clone(),
                    expected_locks: locks.clone(),
                }),
                Some(a) if a != locks => protection_diffs.push(ProtectionDiff::DifferentLock {
                    var: var.clone(),
                    expected: locks.clone(),
                    actual: a.clone(),
                }),
                _ => {}
            }
        }

        Self {
            resource_diffs,
            function_diffs,
            protection_diffs,
        }
    }

    pub fn is_conformant(&self) -> bool {
        self.resource_diffs.is_empty()
            && self.function_diffs.is_empty()
            && self.protection_diffs.is_empty()
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        if !self.resource_diffs.is_empty() {
            s.push_str("Resources:\n");
            for d in &self.resource_diffs {
                s.push_str(&format!("  {:?}\n", d));
            }
        }
        if !self.function_diffs.is_empty() {
            s.push_str("Functions:\n");
            for d in &self.function_diffs {
                s.push_str(&format!("  {:?}\n", d));
            }
        }
        if !self.protection_diffs.is_empty() {
            s.push_str("Protection:\n");
            for d in &self.protection_diffs {
                s.push_str(&format!("  {:?}\n", d));
            }
        }
        s
    }
}

fn op_kind_signature(op: &Option<CirOp>) -> String {
    match op {
        None => "null".into(),
        Some(o) => match o {
            CirOp::Lock { .. } => "lock".into(),
            CirOp::Drop { .. } => "drop".into(),
            CirOp::ReadLock { .. } => "read_lock".into(),
            CirOp::WriteLock { .. } => "write_lock".into(),
            CirOp::Wait { .. } => "wait".into(),
            CirOp::NotifyOne { .. } => "notify_one".into(),
            CirOp::NotifyAll { .. } => "notify_all".into(),
            CirOp::Load { .. } => "load".into(),
            CirOp::Store { .. } => "store".into(),
            CirOp::Cas { .. } => "cas".into(),
            CirOp::Spawn { .. } => "spawn".into(),
            CirOp::Join { .. } => "join".into(),
            CirOp::Call { .. } => "call".into(),
            CirOp::Return => "return".into(),
        },
    }
}

fn align_bodies(
    expected: &[crate::cir::types::CirStatement],
    actual: &[crate::cir::types::CirStatement],
) -> Vec<AlignmentEntry> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < expected.len() || j < actual.len() {
        match (expected.get(i), actual.get(j)) {
            (Some(e), Some(a)) => {
                let se = op_kind_signature(&e.op);
                let sa = op_kind_signature(&a.op);
                if se == sa {
                    out.push(AlignmentEntry::Match {
                        expected_sid: e.sid.clone(),
                        actual_sid: a.sid.clone(),
                    });
                    i += 1;
                    j += 1;
                } else {
                    out.push(AlignmentEntry::OpMismatch {
                        expected_sid: e.sid.clone(),
                        actual_sid: a.sid.clone(),
                        expected_op: se,
                        actual_op: sa,
                    });
                    i += 1;
                    j += 1;
                }
            }
            (Some(e), None) => {
                out.push(AlignmentEntry::ExpectedOnly {
                    sid: e.sid.clone(),
                    op: op_kind_signature(&e.op),
                });
                i += 1;
            }
            (None, Some(a)) => {
                out.push(AlignmentEntry::ActualOnly {
                    sid: a.sid.clone(),
                    op: op_kind_signature(&a.op),
                });
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}
