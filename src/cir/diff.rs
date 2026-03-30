//! Compare expected vs extracted `CirArtifact` (LLM conformance).

use std::collections::BTreeSet;

use crate::cir::types::{CirArtifact, CirFunction, CirResource, ResourceKind};

#[derive(Debug, Clone)]
pub struct CirDiff {
    pub resource_diffs: Vec<ResourceDiff>,
    pub function_diffs: Vec<FunctionDiff>,
    pub protection_diffs: Vec<ProtectionDiff>,
    pub goal_diffs: Vec<GoalDiff>,
}

#[derive(Debug, Clone)]
pub enum ResourceDiff {
    Missing { name: String, kind: ResourceKind },
    Extra { name: String, kind: ResourceKind },
    KindMismatch {
        name: String,
        expected: ResourceKind,
        actual: ResourceKind,
    },
    PairingMismatch {
        name: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone)]
pub enum FunctionDiff {
    MissingFunction { name: String },
    ExtraFunction { name: String },
    BodyDiff {
        function: String,
        expected_ops: Vec<String>,
        actual_ops: Vec<String>,
        alignment: Vec<AlignmentEntry>,
    },
}

#[derive(Debug, Clone)]
pub enum AlignmentEntry {
    Match {
        expected_sid: String,
        actual_sid: String,
    },
    ExpectedOnly { sid: String, op: String },
    ActualOnly { sid: String, op: String },
    Mismatch {
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
    OverProtected {
        var: String,
        extra_locks: Vec<String>,
    },
    DifferentLock {
        var: String,
        expected: Vec<String>,
        actual: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum GoalDiff {
    MissingGoal { id: String },
    ExtraGoal { id: String },
}

impl CirDiff {
    pub fn compare(expected: &CirArtifact, extracted: &CirArtifact) -> Self {
        let mut resource_diffs = Vec::new();
        let mut function_diffs = Vec::new();
        let mut protection_diffs = Vec::new();
        let mut goal_diffs = Vec::new();

        let exp_r: BTreeSet<_> = expected.resources.keys().map(|x| norm_key(x)).collect();
        let ext_r: BTreeSet<_> = extracted.resources.keys().map(|x| norm_key(x)).collect();
        for k in exp_r.difference(&ext_r) {
            let name = orig_key(k.as_str(), &expected.resources);
            let kind = expected.resources[&name].kind.clone();
            resource_diffs.push(ResourceDiff::Missing {
                name,
                kind,
            });
        }
        for k in ext_r.difference(&exp_r) {
            let name = orig_key(k.as_str(), &extracted.resources);
            let kind = extracted.resources[&name].kind.clone();
            resource_diffs.push(ResourceDiff::Extra { name, kind });
        }
        for k in exp_r.intersection(&ext_r) {
            let en = orig_key(k.as_str(), &expected.resources);
            let an = orig_key(k.as_str(), &extracted.resources);
            let e = &expected.resources[&en];
            let a = &extracted.resources[&an];
            if e.kind != a.kind {
                resource_diffs.push(ResourceDiff::KindMismatch {
                    name: en.clone(),
                    expected: e.kind.clone(),
                    actual: a.kind.clone(),
                });
            }
            if e.kind == ResourceKind::Condvar {
                match (&e.paired_with, &a.paired_with) {
                    (Some(ep), Some(ap)) if norm_lock(ep) != norm_lock(ap) => {
                        resource_diffs.push(ResourceDiff::PairingMismatch {
                            name: en.clone(),
                            expected: ep.clone(),
                            actual: ap.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let exp_f: BTreeSet<_> = expected.functions.keys().map(|x| norm_key(x)).collect();
        let ext_f: BTreeSet<_> = extracted.functions.keys().map(|x| norm_key(x)).collect();
        for k in exp_f.difference(&ext_f) {
            function_diffs.push(FunctionDiff::MissingFunction {
                name: orig_fn_key(k.as_str(), &expected.functions),
            });
        }
        for k in ext_f.difference(&exp_f) {
            function_diffs.push(FunctionDiff::ExtraFunction {
                name: orig_fn_key(k.as_str(), &extracted.functions),
            });
        }
        for k in exp_f.intersection(&ext_f) {
            let fk = orig_fn_key(k.as_str(), &expected.functions);
            let ef = &expected.functions[&fk];
            let af = &extracted.functions[&orig_fn_key(k.as_str(), &extracted.functions)];
            let eops: Vec<_> = ef.body.iter().map(|s| s.op.label()).collect();
            let aops: Vec<_> = af.body.iter().map(|s| s.op.label()).collect();
            if eops != aops {
                let alignment = simple_align(&eops, &aops);
                function_diffs.push(FunctionDiff::BodyDiff {
                    function: fk,
                    expected_ops: eops,
                    actual_ops: aops,
                    alignment,
                });
            }
        }

        for (var, elocks) in &expected.protection {
            let nv = norm_key(var);
            let ext_match = extracted
                .protection
                .iter()
                .find(|(v, _)| norm_key(v) == nv)
                .map(|(_, l)| l);
            match ext_match {
                None => protection_diffs.push(ProtectionDiff::Unprotected {
                    var: var.clone(),
                    expected_locks: elocks.clone(),
                }),
                Some(alocks) => {
                    let es: BTreeSet<_> = elocks.iter().map(|s| norm_lock(s)).collect();
                    let a: BTreeSet<_> = alocks.iter().map(|s| norm_lock(s)).collect();
                    if es != a {
                        protection_diffs.push(ProtectionDiff::DifferentLock {
                            var: var.clone(),
                            expected: elocks.clone(),
                            actual: alocks.clone(),
                        });
                    }
                }
            }
        }

        let eg: BTreeSet<_> = expected.goals.iter().map(|g| g.id.clone()).collect();
        let ag: BTreeSet<_> = extracted.goals.iter().map(|g| g.id.clone()).collect();
        for id in eg.difference(&ag) {
            goal_diffs.push(GoalDiff::MissingGoal { id: id.clone() });
        }
        for id in ag.difference(&eg) {
            goal_diffs.push(GoalDiff::ExtraGoal { id: id.clone() });
        }

        Self {
            resource_diffs,
            function_diffs,
            protection_diffs,
            goal_diffs,
        }
    }

    /// Structural conformance: extracted covers expected resources/functions; extras allowed;
    /// protection and goals must match exactly.
    pub fn is_conformant(&self) -> bool {
        self.resource_diffs
            .iter()
            .all(|d| matches!(d, ResourceDiff::Extra { .. }))
            && self
                .function_diffs
                .iter()
                .all(|d| matches!(d, FunctionDiff::ExtraFunction { .. }))
            && self.protection_diffs.is_empty()
            && self.goal_diffs.is_empty()
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        for d in &self.resource_diffs {
            s.push_str(&format!("{:?}\n", d));
        }
        for d in &self.function_diffs {
            s.push_str(&format!("{:?}\n", d));
        }
        for d in &self.protection_diffs {
            s.push_str(&format!("{:?}\n", d));
        }
        for d in &self.goal_diffs {
            s.push_str(&format!("{:?}\n", d));
        }
        s
    }
}

fn norm_key(s: &str) -> String {
    s.to_ascii_lowercase().replace('_', "")
}

fn norm_lock(s: &str) -> String {
    norm_key(s)
}

fn orig_key(norm: &str, m: &std::collections::BTreeMap<String, CirResource>) -> String {
    m.keys()
        .find(|k| norm_key(k) == norm)
        .cloned()
        .unwrap_or_else(|| norm.to_string())
}

fn orig_fn_key(norm: &str, m: &std::collections::BTreeMap<String, CirFunction>) -> String {
    m.keys()
        .find(|k| norm_key(k) == norm)
        .cloned()
        .unwrap_or_else(|| norm.to_string())
}

fn simple_align(expected: &[String], actual: &[String]) -> Vec<AlignmentEntry> {
    let mut out = Vec::new();
    let n = expected.len().max(actual.len());
    for i in 0..n {
        match (expected.get(i), actual.get(i)) {
            (Some(e), Some(a)) if e == a => out.push(AlignmentEntry::Match {
                expected_sid: format!("s{}", i),
                actual_sid: format!("s{}", i),
            }),
            (Some(e), Some(a)) => out.push(AlignmentEntry::Mismatch {
                expected_sid: format!("s{}", i),
                actual_sid: format!("s{}", i),
                expected_op: e.clone(),
                actual_op: a.clone(),
            }),
            (Some(e), None) => out.push(AlignmentEntry::ExpectedOnly {
                sid: format!("s{}", i),
                op: e.clone(),
            }),
            (None, Some(a)) => out.push(AlignmentEntry::ActualOnly {
                sid: format!("s{}", i),
                op: a.clone(),
            }),
            _ => {}
        }
    }
    out
}
