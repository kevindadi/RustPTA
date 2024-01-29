use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DeadlockDiagnosis {
    pub first_lock_type: String,
    pub first_lock_span: String,
    pub second_lock_type: String,
    pub second_lock_span: String,
    pub callchains: Vec<Vec<Vec<String>>>,
}

impl DeadlockDiagnosis {
    pub fn new(
        first_lock_type: String,
        first_lock_span: String,
        second_lock_type: String,
        second_lock_span: String,
        callchains: Vec<Vec<Vec<String>>>,
    ) -> Self {
        Self {
            first_lock_type,
            first_lock_span,
            second_lock_type,
            second_lock_span,
            callchains,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WaitNotifyLocks {
    pub wait_lock_type: String,
    pub wait_lock_span: String,
    pub notify_lock_type: String,
    pub notify_lock_span: String,
}

impl WaitNotifyLocks {
    pub fn new(
        wait_lock_type: String,
        wait_lock_span: String,
        notify_lock_type: String,
        notify_lock_span: String,
    ) -> Self {
        Self {
            wait_lock_type,
            wait_lock_span,
            notify_lock_type,
            notify_lock_span,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CondvarDeadlockDiagnosis {
    pub condvar_wait_type: String,
    pub condvar_wait_callsite_span: String,
    pub condvar_notify_type: String,
    pub condvar_notify_callsite_span: String,
    pub deadlocks: Vec<WaitNotifyLocks>,
}

impl CondvarDeadlockDiagnosis {
    pub fn new(
        condvar_wait_type: String,
        condvar_wait_callsite_span: String,
        condvar_notify_type: String,
        condvar_notify_callsite_span: String,
        deadlocks: Vec<WaitNotifyLocks>,
    ) -> Self {
        Self {
            condvar_wait_type,
            condvar_wait_callsite_span,
            condvar_notify_type,
            condvar_notify_callsite_span,
            deadlocks,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct ReportContent<D> {
    pub bug_kind: String,
    pub possibility: String,
    pub diagnosis: D,
    pub explanation: String,
}

impl<D: std::fmt::Debug> ReportContent<D> {
    pub fn new(bug_kind: String, possibility: String, diagnosis: D, explanation: String) -> Self {
        Self {
            bug_kind,
            possibility,
            diagnosis,
            explanation,
        }
    }
}

// #[derive(Debug, Serialize)]
// pub enum Report {
//     DoubleLock(ReportContent<DeadlockDiagnosis>),
//     ConflictLock(ReportContent<Vec<DeadlockDiagnosis>>),
//     CondvarDeadlock(ReportContent<CondvarDeadlockDiagnosis>),
//     InvalidFree(ReportContent<String>),
//     UseAfterFree(ReportContent<String>),
// }

// pub fn report_stats(crate_name: &str, reports: &[Report]) -> String {
//     let (
//         mut doublelock_probably,
//         mut doublelock_possibly,
//         mut conflictlock_probably,
//         mut conflictlock_possibly,
//         mut condvar_deadlock_probably,
//         mut condvar_deadlock_possibly,
//         mut atomicity_violation_possibly,
//         mut invalid_free_possibly,
//         mut use_after_free_possibly,
//     ) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
//     for report in reports {
//         match report {
//             Report::DoubleLock(doublelock) => match doublelock.possibility.as_str() {
//                 "Probably" => doublelock_probably += 1,
//                 "Possibly" => doublelock_possibly += 1,
//                 _ => {}
//             },
//             Report::ConflictLock(conflictlock) => match conflictlock.possibility.as_str() {
//                 "Probably" => conflictlock_probably += 1,
//                 "Possibly" => conflictlock_possibly += 1,
//                 _ => {}
//             },
//             Report::CondvarDeadlock(condvar_deadlock) => {
//                 match condvar_deadlock.possibility.as_str() {
//                     "Probably" => condvar_deadlock_probably += 1,
//                     "Possibly" => condvar_deadlock_possibly += 1,
//                     _ => {}
//                 }
//             }
//             Report::InvalidFree(_) => {
//                 invalid_free_possibly += 1;
//             }
//             Report::UseAfterFree(_) => {
//                 use_after_free_possibly += 1;
//             }
//         }
//     }
//     format!("crate {} contains bugs: {{ probably: {}, possibly: {} }}, conflictlock: {{ probably: {}, possibly: {} }}, condvar_deadlock: {{ probably: {}, possibly: {} }}, atomicity_violation: {{ possibly: {} }}, invalid_free: {{ possibly: {} }}, use_after_free: {{ possibly: {} }}", crate_name, doublelock_probably, doublelock_possibly, conflictlock_probably, conflictlock_possibly, condvar_deadlock_probably, condvar_deadlock_possibly, atomicity_violation_possibly, invalid_free_possibly, use_after_free_possibly)
// }