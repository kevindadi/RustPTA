//! Integration tests: hand-built `Net` → `CirExtractor` (naming aligned with `{name}_start` / `{name}_end`).

#![feature(rustc_private)]

use RustPTA::cir::diff::{CirDiff, ResourceDiff};
use RustPTA::cir::extractor::CirExtractor;
use RustPTA::cir::integration::extract_and_verify;
use RustPTA::cir::types::{
    CirArtifact, CirOp, CirResource, CirTransfer, FunctionKind, ResourceKind,
};
use RustPTA::net::Idx;
use RustPTA::net::core::Net;
use RustPTA::net::structure::{Place, PlaceType, Transition, TransitionType};

fn span() -> String {
    String::new()
}

fn p(
    net: &mut Net,
    name: &str,
    tokens: u64,
    cap: u64,
    ty: PlaceType,
) -> RustPTA::net::ids::PlaceId {
    net.add_place(Place::new(name, tokens, cap, ty, span()))
}

fn t(net: &mut Net, name: &str, ty: TransitionType) -> RustPTA::net::ids::TransitionId {
    net.add_transition(Transition::new_with_transition_type(name, ty))
}

fn wire(
    net: &mut Net,
    from_p: RustPTA::net::ids::PlaceId,
    trans: RustPTA::net::ids::TransitionId,
    to_p: RustPTA::net::ids::PlaceId,
) {
    net.add_input_arc(from_p, trans, 1);
    net.add_output_arc(to_p, trans, 1);
}

/// Minimal `main` only: start → bb → end (Goto chain).
fn net_minimal_main() -> Net {
    let mut net = Net::empty();
    let main_start = p(
        &mut net,
        "test_mod::main_start",
        1,
        10,
        PlaceType::FunctionStart,
    );
    let main_0 = p(&mut net, "test_mod::main_0", 0, 10, PlaceType::BasicBlock);
    let main_end = p(
        &mut net,
        "test_mod::main_end",
        0,
        10,
        PlaceType::FunctionEnd,
    );
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    wire(&mut net, main_0, g1, main_end);
    net
}

/// Two-thread style: `main` spawns/joins `thread_a` and `thread_b`; each thread lock/unlocks `m0`.
fn build_two_mutex_net() -> Net {
    let mut net = Net::empty();
    let m_free = p(&mut net, "mutex_r0", 1, 1, PlaceType::Resources);

    let main_start = p(
        &mut net,
        "test_mod::main_start",
        1,
        10,
        PlaceType::FunctionStart,
    );
    let main_0 = p(&mut net, "test_mod::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "test_mod::main_1", 0, 10, PlaceType::BasicBlock);
    let main_2 = p(&mut net, "test_mod::main_2", 0, 10, PlaceType::BasicBlock);
    let main_3 = p(&mut net, "test_mod::main_3", 0, 10, PlaceType::BasicBlock);
    let main_4 = p(&mut net, "test_mod::main_4", 0, 10, PlaceType::BasicBlock);
    let main_end = p(
        &mut net,
        "test_mod::main_end",
        0,
        10,
        PlaceType::FunctionEnd,
    );

    let ta_start = p(
        &mut net,
        "test_mod::thread_a_start",
        0,
        10,
        PlaceType::FunctionStart,
    );
    let ta_0 = p(
        &mut net,
        "test_mod::thread_a_0",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let ta_1 = p(
        &mut net,
        "test_mod::thread_a_1",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let ta_2 = p(
        &mut net,
        "test_mod::thread_a_2",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let ta_end = p(
        &mut net,
        "test_mod::thread_a_end",
        0,
        10,
        PlaceType::FunctionEnd,
    );

    let tb_start = p(
        &mut net,
        "test_mod::thread_b_start",
        0,
        10,
        PlaceType::FunctionStart,
    );
    let tb_0 = p(
        &mut net,
        "test_mod::thread_b_0",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let tb_1 = p(
        &mut net,
        "test_mod::thread_b_1",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let tb_2 = p(
        &mut net,
        "test_mod::thread_b_2",
        0,
        10,
        PlaceType::BasicBlock,
    );
    let tb_end = p(
        &mut net,
        "test_mod::thread_b_end",
        0,
        10,
        PlaceType::FunctionEnd,
    );

    let g_m0 = t(&mut net, "mg0", TransitionType::Goto);
    let sp_a = t(
        &mut net,
        "sp_a",
        TransitionType::Spawn("test_mod::thread_a".into()),
    );
    let sp_b = t(
        &mut net,
        "sp_b",
        TransitionType::Spawn("test_mod::thread_b".into()),
    );
    let j_a = t(
        &mut net,
        "j_a",
        TransitionType::Join("test_mod::thread_a".into()),
    );
    let j_b = t(
        &mut net,
        "j_b",
        TransitionType::Join("test_mod::thread_b".into()),
    );
    let g_m1 = t(&mut net, "mg1", TransitionType::Goto);

    wire(&mut net, main_start, g_m0, main_0);
    wire(&mut net, main_0, sp_a, main_1);
    wire(&mut net, main_1, sp_b, main_2);
    wire(&mut net, main_2, j_a, main_3);
    wire(&mut net, main_3, j_b, main_4);
    wire(&mut net, main_4, g_m1, main_end);

    let ta_g0 = t(&mut net, "tag0", TransitionType::Goto);
    let ta_lk = t(&mut net, "talock", TransitionType::Lock(0));
    let ta_un = t(&mut net, "taunlock", TransitionType::Unlock(0));
    let ta_g1 = t(&mut net, "tag1", TransitionType::Goto);
    wire(&mut net, ta_start, ta_g0, ta_0);
    net.add_input_arc(ta_0, ta_lk, 1);
    net.add_input_arc(m_free, ta_lk, 1);
    net.add_output_arc(ta_1, ta_lk, 1);
    net.add_input_arc(ta_1, ta_un, 1);
    net.add_input_arc(m_free, ta_un, 1);
    net.add_output_arc(ta_2, ta_un, 1);
    wire(&mut net, ta_2, ta_g1, ta_end);

    let tb_g0 = t(&mut net, "tbg0", TransitionType::Goto);
    let tb_lk = t(&mut net, "tblock", TransitionType::Lock(0));
    let tb_un = t(&mut net, "tbunlock", TransitionType::Unlock(0));
    let tb_g1 = t(&mut net, "tbg1", TransitionType::Goto);
    wire(&mut net, tb_start, tb_g0, tb_0);
    net.add_input_arc(tb_0, tb_lk, 1);
    net.add_input_arc(m_free, tb_lk, 1);
    net.add_output_arc(tb_1, tb_lk, 1);
    net.add_input_arc(tb_1, tb_un, 1);
    net.add_input_arc(m_free, tb_un, 1);
    net.add_output_arc(tb_2, tb_un, 1);
    wire(&mut net, tb_2, tb_g1, tb_end);

    net
}

#[test]
fn test_01_yaml_roundtrip_empty_protection() {
    let net = net_minimal_main();
    let art = CirExtractor::new(&net).extract().expect("extract");
    let y = art.to_yaml().unwrap();
    let back: CirArtifact = CirArtifact::from_yaml(&y).unwrap();
    assert_eq!(art.entry, back.entry);
    assert_eq!(art.resources, back.resources);
    assert_eq!(
        art.functions.keys().collect::<Vec<_>>(),
        back.functions.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_02_extract_two_mutex_net_has_three_goals() {
    let net = build_two_mutex_net();
    let art = CirExtractor::new(&net).extract().expect("extract");
    assert_eq!(art.entry, "main");
    assert!(
        art.functions.contains_key("main")
            && art.functions.contains_key("thread_a")
            && art.functions.contains_key("thread_b")
    );
    let ids: Vec<_> = art.goals.iter().map(|g| g.id.as_str()).collect();
    assert!(ids.iter().any(|i| *i == "G0"), "main goal: {:?}", art.goals);
    assert!(
        ids.iter().any(|i| *i == "G1") && ids.iter().any(|i| *i == "G2"),
        "spawn goals: {:?}",
        art.goals
    );
}

#[test]
fn test_03_mutex_resource_named_m0() {
    let mut net = Net::empty();
    let m_free = p(&mut net, "mx0", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "m::main_start", 1, 10, PlaceType::FunctionStart);
    let main_0 = p(&mut net, "m::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "m::main_1", 0, 10, PlaceType::BasicBlock);
    let main_2 = p(&mut net, "m::main_2", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "m::main_end", 0, 10, PlaceType::FunctionEnd);
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let lk = t(&mut net, "lk", TransitionType::Lock(0));
    let ul = t(&mut net, "ul", TransitionType::Unlock(0));
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    net.add_input_arc(main_0, lk, 1);
    net.add_input_arc(m_free, lk, 1);
    net.add_output_arc(main_1, lk, 1);
    net.add_input_arc(main_1, ul, 1);
    net.add_input_arc(m_free, ul, 1);
    net.add_output_arc(main_2, ul, 1);
    wire(&mut net, main_2, g1, main_end);

    let art = CirExtractor::new(&net).extract().expect("extract");
    assert!(art.resources.contains_key("m0"));
    assert_eq!(art.resources["m0"].kind, ResourceKind::Mutex);
}

#[test]
fn test_04_diff_identical_conformant() {
    let net = net_minimal_main();
    let a = CirExtractor::new(&net).extract().unwrap();
    let d = CirDiff::compare(&a, &a);
    assert!(d.is_conformant());
    assert!(d.report().trim().is_empty());
}

#[test]
fn test_05_diff_detects_missing_resource() {
    let net = net_minimal_main();
    let got = CirExtractor::new(&net).extract().unwrap();
    let mut expected = got.clone();
    expected.resources.insert(
        "ghost_lock".into(),
        CirResource {
            kind: ResourceKind::Mutex,
            paired_with: None,
            permits: None,
            capacity: None,
            var_type: None,
            init: None,
            span: None,
        },
    );
    let d = CirDiff::compare(&expected, &got);
    assert!(!d.is_conformant());
    assert!(
        d.resource_diffs
            .iter()
            .any(|r| matches!(r, ResourceDiff::Missing { .. }))
    );
}

#[test]
fn test_06_integration_verify_false_on_missing_expected_resource() {
    let net = net_minimal_main();
    let mut phantom = CirExtractor::new(&net).extract().unwrap();
    phantom.resources.insert(
        "x".into(),
        CirResource {
            kind: ResourceKind::Atomic,
            paired_with: None,
            permits: None,
            capacity: None,
            var_type: None,
            init: None,
            span: None,
        },
    );
    let res = extract_and_verify(&net, Some(&phantom));
    assert!(!res.conformant);
}

#[test]
fn test_07_anchor_ret_maps_to_function_end_place_index() {
    let net = net_minimal_main();
    let art = CirExtractor::new(&net).extract().unwrap();
    art.functions["main"]
        .body
        .iter()
        .position(|s| s.sid == "ret")
        .expect("ret sid");
    let ret_place = *art.anchor_map.sid_to_place.get("ret").expect("ret anchor");
    assert!(
        art.anchor_map.place_to_sid.get(&ret_place).is_some(),
        "reverse anchor for place {}",
        ret_place
    );
}

#[test]
fn test_08_rwlock_ops() {
    let mut net = Net::empty();
    let rw_free = p(&mut net, "rw_p", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "r::main_start", 1, 10, PlaceType::FunctionStart);
    let main_0 = p(&mut net, "r::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "r::main_1", 0, 10, PlaceType::BasicBlock);
    let main_2 = p(&mut net, "r::main_2", 0, 10, PlaceType::BasicBlock);
    let main_3 = p(&mut net, "r::main_3", 0, 10, PlaceType::BasicBlock);
    let main_4 = p(&mut net, "r::main_4", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "r::main_end", 0, 10, PlaceType::FunctionEnd);
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let rdr = t(&mut net, "rdr", TransitionType::RwLockRead(0));
    let drp = t(&mut net, "drp", TransitionType::DropRead(0));
    let wlk = t(&mut net, "wlk", TransitionType::RwLockWrite(0));
    let dwp = t(&mut net, "dwp", TransitionType::DropWrite(0));
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    net.add_input_arc(main_0, rdr, 1);
    net.add_input_arc(rw_free, rdr, 1);
    net.add_output_arc(main_1, rdr, 1);
    net.add_input_arc(main_1, drp, 1);
    net.add_input_arc(rw_free, drp, 1);
    net.add_output_arc(main_2, drp, 1);
    net.add_input_arc(main_2, wlk, 1);
    net.add_input_arc(rw_free, wlk, 1);
    net.add_output_arc(main_3, wlk, 1);
    net.add_input_arc(main_3, dwp, 1);
    net.add_input_arc(rw_free, dwp, 1);
    net.add_output_arc(main_4, dwp, 1);
    wire(&mut net, main_4, g1, main_end);

    let art = CirExtractor::new(&net).extract().expect("extract");
    assert!(art.resources.contains_key("rw0"));
    let labels: Vec<_> = art.functions["main"]
        .body
        .iter()
        .map(|s| s.op.label())
        .collect();
    assert!(labels.iter().any(|l| l == "read_lock"));
    assert!(labels.iter().any(|l| l == "write_lock"));
    assert!(labels.iter().any(|l| l == "drop"));
}

#[test]
fn test_09_notify_one_emitted() {
    let mut net = Net::empty();
    let cv_tok = p(&mut net, "cv_tok", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "n::main_start", 1, 10, PlaceType::FunctionStart);
    let main_0 = p(&mut net, "n::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "n::main_1", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "n::main_end", 0, 10, PlaceType::FunctionEnd);
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let nv = t(&mut net, "nv", TransitionType::Notify(0));
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    net.add_input_arc(cv_tok, nv, 1);
    net.add_input_arc(main_0, nv, 1);
    net.add_output_arc(main_1, nv, 1);
    wire(&mut net, main_1, g1, main_end);

    let art = CirExtractor::new(&net).extract().expect("extract");
    assert!(
        art.functions["main"]
            .body
            .iter()
            .any(|s| matches!(s.op, CirOp::NotifyOne { .. }))
    );
}

#[test]
fn test_10_protection_unsafe_read_under_lock() {
    let mut net = Net::empty();
    let m_free = p(&mut net, "m10", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "p::main_start", 1, 10, PlaceType::FunctionStart);
    let main_0 = p(&mut net, "p::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "p::main_1", 0, 10, PlaceType::BasicBlock);
    let main_2 = p(&mut net, "p::main_2", 0, 10, PlaceType::BasicBlock);
    let main_3 = p(&mut net, "p::main_3", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "p::main_end", 0, 10, PlaceType::FunctionEnd);
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let lk = t(&mut net, "lk", TransitionType::Lock(0));
    let rd = t(
        &mut net,
        "rd",
        TransitionType::UnsafeRead(7, "v7".into(), 0, String::new()),
    );
    let ul = t(&mut net, "ul", TransitionType::Unlock(0));
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    net.add_input_arc(main_0, lk, 1);
    net.add_input_arc(m_free, lk, 1);
    net.add_output_arc(main_1, lk, 1);
    net.add_input_arc(main_1, rd, 1);
    net.add_output_arc(main_2, rd, 1);
    net.add_input_arc(main_2, ul, 1);
    net.add_input_arc(m_free, ul, 1);
    net.add_output_arc(main_3, ul, 1);
    wire(&mut net, main_3, g1, main_end);

    let art = CirExtractor::new(&net).extract().expect("extract");
    assert_eq!(art.protection.get("v7"), Some(&vec!["m0".to_string()]));
}

#[test]
fn test_11_wait_pairs_condvar() {
    let mut net = Net::empty();
    let m_free = p(&mut net, "m_w", 1, 1, PlaceType::Resources);
    let cv_tok = p(&mut net, "cv_w", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "w::main_start", 1, 10, PlaceType::FunctionStart);
    let bb_a = p(&mut net, "w::main_0", 0, 10, PlaceType::BasicBlock);
    let bb_b = p(&mut net, "w::main_1", 0, 10, PlaceType::BasicBlock);
    let bb_c = p(&mut net, "w::main_2", 0, 10, PlaceType::BasicBlock);
    let bb_d = p(&mut net, "w::main_3", 0, 10, PlaceType::BasicBlock);
    let bb_e = p(&mut net, "w::main_4", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "w::main_end", 0, 10, PlaceType::FunctionEnd);

    let g0 = t(&mut net, "wg0", TransitionType::Goto);
    let lk = t(&mut net, "wlk", TransitionType::Lock(0));
    let ul = t(&mut net, "wul", TransitionType::Unlock(0));
    let nv = t(&mut net, "wnv", TransitionType::Notify(1));
    let w = t(&mut net, "wwait", TransitionType::Wait);
    let g1 = t(&mut net, "wg1", TransitionType::Goto);

    wire(&mut net, main_start, g0, bb_a);
    net.add_input_arc(bb_a, lk, 1);
    net.add_input_arc(m_free, lk, 1);
    net.add_output_arc(bb_b, lk, 1);
    net.add_input_arc(bb_b, ul, 1);
    net.add_input_arc(m_free, ul, 1);
    net.add_output_arc(bb_c, ul, 1);

    net.add_input_arc(cv_tok, nv, 1);
    net.add_input_arc(bb_c, nv, 1);
    net.add_output_arc(bb_d, nv, 1);

    net.add_input_arc(bb_d, w, 1);
    net.add_input_arc(cv_tok, w, 1);
    net.add_output_arc(bb_e, w, 1);
    net.add_output_arc(m_free, w, 1);

    wire(&mut net, bb_e, g1, main_end);

    let art = CirExtractor::new(&net).extract().expect("extract");
    assert_eq!(
        art.resources.get("cv0").unwrap().paired_with.as_deref(),
        Some("m0")
    );
    assert!(
        art.functions["main"]
            .body
            .iter()
            .any(|s| matches!(s.op, CirOp::Wait { .. }))
    );
}

#[test]
fn test_12_empty_net_errors() {
    let net = Net::empty();
    let r = CirExtractor::new(&net).extract();
    assert!(r.is_err());
}

#[test]
fn test_13_entry_is_main_when_token_on_main_start() {
    let net = net_minimal_main();
    let art = CirExtractor::new(&net).extract().unwrap();
    assert_eq!(art.entry, "main");
    assert_eq!(art.functions["main"].kind, FunctionKind::Normal);
}

#[test]
fn test_14_resource_to_places_in_anchor() {
    let mut net = Net::empty();
    let m_free = p(&mut net, "mrp", 1, 1, PlaceType::Resources);
    let main_start = p(&mut net, "rp::main_start", 1, 10, PlaceType::FunctionStart);
    let main_0 = p(&mut net, "rp::main_0", 0, 10, PlaceType::BasicBlock);
    let main_1 = p(&mut net, "rp::main_1", 0, 10, PlaceType::BasicBlock);
    let main_2 = p(&mut net, "rp::main_2", 0, 10, PlaceType::BasicBlock);
    let main_end = p(&mut net, "rp::main_end", 0, 10, PlaceType::FunctionEnd);
    let g0 = t(&mut net, "g0", TransitionType::Goto);
    let lk = t(&mut net, "lk", TransitionType::Lock(0));
    let ul = t(&mut net, "ul", TransitionType::Unlock(0));
    let g1 = t(&mut net, "g1", TransitionType::Goto);
    wire(&mut net, main_start, g0, main_0);
    net.add_input_arc(main_0, lk, 1);
    net.add_input_arc(m_free, lk, 1);
    net.add_output_arc(main_1, lk, 1);
    net.add_input_arc(main_1, ul, 1);
    net.add_input_arc(m_free, ul, 1);
    net.add_output_arc(main_2, ul, 1);
    wire(&mut net, main_2, g1, main_end);

    let art = CirExtractor::new(&net).extract().unwrap();
    let places = art
        .anchor_map
        .resource_to_places
        .get("m0")
        .cloned()
        .unwrap_or_default();
    assert!(places.contains(&m_free.index()));
}

#[test]
fn test_15_merge_body_transfer_chain_ends_with_return_done() {
    let net = net_minimal_main();
    let art = CirExtractor::new(&net).extract().unwrap();
    let body = &art.functions["main"].body;
    let last = body.last().expect("body");
    assert!(matches!(last.op, CirOp::Return));
    assert!(matches!(last.transfer, CirTransfer::Done { done: true }));
}
