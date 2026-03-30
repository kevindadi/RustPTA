//! Unit tests (live under `src/` so `rustc_private` linking matches the library).
use crate::cir::diff::CirDiff;
use crate::cir::net_extract::CirExtractor;
use crate::cir::types::{CirOp, CirTransfer, ResourceKind};
use crate::net::core::Net;
use crate::net::structure::{Place, PlaceType, Transition, TransitionType};

fn build_two_mutex_net() -> Net {
    let mut net = Net::empty();

    let p_m0 = net.add_place(Place::new(
        "mutex_0",
        1,
        1,
        PlaceType::Resources,
        "src/main.rs:5".into(),
    ));
    let p_m1 = net.add_place(Place::new(
        "mutex_1",
        1,
        1,
        PlaceType::Resources,
        "src/main.rs:6".into(),
    ));

    let p_main_start = net.add_place(Place::new(
        "fn_main_start",
        1,
        1,
        PlaceType::FunctionStart,
        "".into(),
    ));
    let p_main_bb1 = net.add_place(Place::new(
        "fn_main_bb1",
        0,
        1,
        PlaceType::BasicBlock,
        "".into(),
    ));
    let p_main_bb2 = net.add_place(Place::new(
        "fn_main_bb2",
        0,
        1,
        PlaceType::BasicBlock,
        "".into(),
    ));
    let p_main_bb3 = net.add_place(Place::new(
        "fn_main_bb3",
        0,
        1,
        PlaceType::BasicBlock,
        "".into(),
    ));
    let p_main_bb4 = net.add_place(Place::new(
        "fn_main_bb4",
        0,
        1,
        PlaceType::BasicBlock,
        "".into(),
    ));
    let p_main_end = net.add_place(Place::new(
        "fn_main_end",
        0,
        1,
        PlaceType::FunctionEnd,
        "".into(),
    ));

    let p_a_start = net.add_place(Place::new(
        "fn_thread_a_start",
        0,
        1,
        PlaceType::FunctionStart,
        "".into(),
    ));
    let p_a_bb1 = net.add_place(Place::new(
        "fn_thread_a_bb1",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:10".into(),
    ));
    let p_a_bb2 = net.add_place(Place::new(
        "fn_thread_a_bb2",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:11".into(),
    ));
    let p_a_bb3 = net.add_place(Place::new(
        "fn_thread_a_bb3",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:12".into(),
    ));
    let p_a_end = net.add_place(Place::new(
        "fn_thread_a_end",
        0,
        1,
        PlaceType::FunctionEnd,
        "".into(),
    ));

    let p_b_start = net.add_place(Place::new(
        "fn_thread_b_start",
        0,
        1,
        PlaceType::FunctionStart,
        "".into(),
    ));
    let p_b_bb1 = net.add_place(Place::new(
        "fn_thread_b_bb1",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:20".into(),
    ));
    let p_b_bb2 = net.add_place(Place::new(
        "fn_thread_b_bb2",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:21".into(),
    ));
    let p_b_bb3 = net.add_place(Place::new(
        "fn_thread_b_bb3",
        0,
        1,
        PlaceType::BasicBlock,
        "src/main.rs:22".into(),
    ));
    let p_b_end = net.add_place(Place::new(
        "fn_thread_b_end",
        0,
        1,
        PlaceType::FunctionEnd,
        "".into(),
    ));

    let t_spawn_a = net.add_transition(Transition::new_with_transition_type(
        "spawn_a",
        TransitionType::Spawn("thread_a".into()),
    ));
    net.add_input_arc(p_main_start, t_spawn_a, 1);
    net.add_output_arc(p_main_bb1, t_spawn_a, 1);
    net.add_output_arc(p_a_start, t_spawn_a, 1);

    let t_spawn_b = net.add_transition(Transition::new_with_transition_type(
        "spawn_b",
        TransitionType::Spawn("thread_b".into()),
    ));
    net.add_input_arc(p_main_bb1, t_spawn_b, 1);
    net.add_output_arc(p_main_bb2, t_spawn_b, 1);
    net.add_output_arc(p_b_start, t_spawn_b, 1);

    let t_join_a = net.add_transition(Transition::new_with_transition_type(
        "join_a",
        TransitionType::Join("thread_a".into()),
    ));
    net.add_input_arc(p_main_bb2, t_join_a, 1);
    net.add_input_arc(p_a_end, t_join_a, 1);
    net.add_output_arc(p_main_bb3, t_join_a, 1);

    let t_join_b = net.add_transition(Transition::new_with_transition_type(
        "join_b",
        TransitionType::Join("thread_b".into()),
    ));
    net.add_input_arc(p_main_bb3, t_join_b, 1);
    net.add_input_arc(p_b_end, t_join_b, 1);
    net.add_output_arc(p_main_bb4, t_join_b, 1);

    let t_main_ret = net.add_transition(Transition::new_with_transition_type(
        "main_ret",
        TransitionType::Return(0),
    ));
    net.add_input_arc(p_main_bb4, t_main_ret, 1);
    net.add_output_arc(p_main_end, t_main_ret, 1);

    let t_a_lock0 = net.add_transition(Transition::new_with_transition_type(
        "a_lock_m0",
        TransitionType::Lock(0),
    ));
    net.add_input_arc(p_a_start, t_a_lock0, 1);
    net.add_input_arc(p_m0, t_a_lock0, 1);
    net.add_output_arc(p_a_bb1, t_a_lock0, 1);

    let t_a_lock1 = net.add_transition(Transition::new_with_transition_type(
        "a_lock_m1",
        TransitionType::Lock(1),
    ));
    net.add_input_arc(p_a_bb1, t_a_lock1, 1);
    net.add_input_arc(p_m1, t_a_lock1, 1);
    net.add_output_arc(p_a_bb2, t_a_lock1, 1);

    let t_a_unlock1 = net.add_transition(Transition::new_with_transition_type(
        "a_unlock_m1",
        TransitionType::Unlock(1),
    ));
    net.add_input_arc(p_a_bb2, t_a_unlock1, 1);
    net.add_output_arc(p_a_bb3, t_a_unlock1, 1);
    net.add_output_arc(p_m1, t_a_unlock1, 1);

    let t_a_unlock0 = net.add_transition(Transition::new_with_transition_type(
        "a_unlock_m0",
        TransitionType::Unlock(0),
    ));
    net.add_input_arc(p_a_bb3, t_a_unlock0, 1);
    net.add_output_arc(p_a_end, t_a_unlock0, 1);
    net.add_output_arc(p_m0, t_a_unlock0, 1);

    let t_b_lock1 = net.add_transition(Transition::new_with_transition_type(
        "b_lock_m1",
        TransitionType::Lock(1),
    ));
    net.add_input_arc(p_b_start, t_b_lock1, 1);
    net.add_input_arc(p_m1, t_b_lock1, 1);
    net.add_output_arc(p_b_bb1, t_b_lock1, 1);

    let t_b_lock0 = net.add_transition(Transition::new_with_transition_type(
        "b_lock_m0",
        TransitionType::Lock(0),
    ));
    net.add_input_arc(p_b_bb1, t_b_lock0, 1);
    net.add_input_arc(p_m0, t_b_lock0, 1);
    net.add_output_arc(p_b_bb2, t_b_lock0, 1);

    let t_b_unlock0 = net.add_transition(Transition::new_with_transition_type(
        "b_unlock_m0",
        TransitionType::Unlock(0),
    ));
    net.add_input_arc(p_b_bb2, t_b_unlock0, 1);
    net.add_output_arc(p_b_bb3, t_b_unlock0, 1);
    net.add_output_arc(p_m0, t_b_unlock0, 1);

    let t_b_unlock1 = net.add_transition(Transition::new_with_transition_type(
        "b_unlock_m1",
        TransitionType::Unlock(1),
    ));
    net.add_input_arc(p_b_bb3, t_b_unlock1, 1);
    net.add_output_arc(p_b_end, t_b_unlock1, 1);
    net.add_output_arc(p_m1, t_b_unlock1, 1);

    net
}

#[test]
fn test_01_resource_extraction() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let mutexes: Vec<_> = cir
        .resources
        .iter()
        .filter(|(_, r)| r.kind == ResourceKind::Mutex)
        .collect();
    assert_eq!(mutexes.len(), 2, "Should find exactly 2 mutexes");
}

#[test]
fn test_02_function_discovery() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    assert_eq!(cir.functions.len(), 3);
    assert!(cir.functions.contains_key("main"));
    assert!(cir.functions.contains_key("thread_a"));
    assert!(cir.functions.contains_key("thread_b"));
}

#[test]
fn test_03_lock_order_thread_a() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let body = &cir.functions["thread_a"].body;
    let ops: Vec<_> = body.iter().map(|s| format!("{:?}", s.op)).collect();
    assert!(ops[0].contains("Lock") && ops[0].contains("m0"));
    assert!(ops[1].contains("Lock") && ops[1].contains("m1"));
    assert!(ops[2].contains("Drop") && ops[2].contains("m1"));
    assert!(ops[3].contains("Drop") && ops[3].contains("m0"));
}

#[test]
fn test_04_lock_order_thread_b_opposite() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let body = &cir.functions["thread_b"].body;
    let ops: Vec<_> = body.iter().map(|s| format!("{:?}", s.op)).collect();
    assert!(ops[0].contains("Lock") && ops[0].contains("m1"));
    assert!(ops[1].contains("Lock") && ops[1].contains("m0"));
}

#[test]
fn test_05_main_spawn_join() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let body = &cir.functions["main"].body;
    let ops: Vec<_> = body.iter().map(|s| format!("{:?}", s.op)).collect();
    assert!(ops[0].contains("Spawn") && ops[0].contains("thread_a"));
    assert!(ops[1].contains("Spawn") && ops[1].contains("thread_b"));
    assert!(ops[2].contains("Join") && ops[2].contains("thread_a"));
    assert!(ops[3].contains("Join") && ops[3].contains("thread_b"));
}

#[test]
fn test_06_transfer_chain() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    for func in cir.functions.values() {
        for i in 0..func.body.len().saturating_sub(1) {
            if let CirTransfer::Next { next } = &func.body[i].transfer {
                assert_eq!(next, &func.body[i + 1].sid);
            }
        }
        let last = func.body.last().expect("last");
        assert!(matches!(last.transfer, CirTransfer::Done { .. }));
    }
}

#[test]
fn test_07_entry_is_main() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    assert_eq!(cir.entry, "main");
}

#[test]
fn test_08_goals() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    assert!(cir
        .goals
        .iter()
        .any(|g| g.marking.contains_key("cp(thread_a, ret)")));
    assert!(cir
        .goals
        .iter()
        .any(|g| g.marking.contains_key("cp(thread_b, ret)")));
}

#[test]
fn test_09_same_resource_id_same_name() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let a_ops: Vec<_> = cir.functions["thread_a"]
        .body
        .iter()
        .filter_map(|s| match &s.op {
            Some(CirOp::Lock { lock }) => Some(lock.clone()),
            _ => None,
        })
        .collect();
    let b_ops: Vec<_> = cir.functions["thread_b"]
        .body
        .iter()
        .filter_map(|s| match &s.op {
            Some(CirOp::Lock { lock }) => Some(lock.clone()),
            _ => None,
        })
        .collect();
    assert!(a_ops.contains(&"m0".to_string()));
    assert!(a_ops.contains(&"m1".to_string()));
    assert!(b_ops.contains(&"m0".to_string()));
    assert!(b_ops.contains(&"m1".to_string()));
}

#[test]
fn test_10_yaml_roundtrip() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let yaml = cir.to_yaml().expect("yaml");
    let cir2 = crate::cir::types::CirArtifact::from_yaml(&yaml).expect("parse");
    assert_eq!(cir.resources, cir2.resources);
    assert_eq!(cir.functions, cir2.functions);
    assert_eq!(cir.entry, cir2.entry);
}

#[test]
fn test_11_diff_conformant() {
    let net = build_two_mutex_net();
    let cir = CirExtractor::new(&net).extract().expect("cir");
    let diff = CirDiff::compare(&cir, &cir);
    assert!(diff.is_conformant());
}

#[test]
fn test_12_diff_detects_missing_resource() {
    let net = build_two_mutex_net();
    let extracted = CirExtractor::new(&net).extract().expect("cir");
    let mut expected = extracted.clone();
    expected.resources.insert(
        "m2".into(),
        crate::cir::types::CirResource {
            kind: ResourceKind::Mutex,
            paired_with: None,
            span: None,
        },
    );
    let diff = CirDiff::compare(&expected, &extracted);
    assert!(!diff.is_conformant());
}
