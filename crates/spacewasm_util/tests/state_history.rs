use spacewasm::JumpTarget;
use spacewasm_util::{RustSystemAllocator, StateHistory, StateSnapshot};

spacewasm::global_allocator!(RustSystemAllocator, RustSystemAllocator);

fn snap(pc: u32, instruction: &'static str) -> StateSnapshot {
    StateSnapshot {
        pc: JumpTarget(pc),
        sp: pc as usize,
        fp: 0,
        instruction,
        metadata: None,
    }
}

fn pcs(history: &StateHistory) -> Vec<u32> {
    history.iter().map(|s| s.pc.0).collect()
}

#[test]
fn keeps_insertion_order_below_capacity() {
    let mut h = StateHistory::new(4);
    for i in 0..3 {
        h.record(snap(i, "nop"));
    }
    assert_eq!(pcs(&h), vec![0, 1, 2]);
}

#[test]
fn keeps_insertion_order_at_capacity() {
    let mut h = StateHistory::new(3);
    for i in 0..3 {
        h.record(snap(i, "nop"));
    }
    assert_eq!(pcs(&h), vec![0, 1, 2]);
}

#[test]
fn drops_oldest_once_wrapped() {
    let mut h = StateHistory::new(3);
    for i in 0..5 {
        h.record(snap(i, "nop"));
    }
    assert_eq!(pcs(&h), vec![2, 3, 4]);
}

#[test]
fn stays_correct_across_several_wraps() {
    let mut h = StateHistory::new(3);
    for i in 0..10 {
        h.record(snap(i, "nop"));
    }
    assert_eq!(pcs(&h), vec![7, 8, 9]);
}

#[test]
fn capacity_one_keeps_only_the_latest() {
    let mut h = StateHistory::new(1);
    for i in 0..4 {
        h.record(snap(i, "nop"));
    }
    assert_eq!(pcs(&h), vec![3]);
}

// `--limit 0` reaches this and used to panic.
#[test]
fn zero_capacity_records_nothing() {
    let mut h = StateHistory::new(0);
    h.record(snap(1, "nop"));
    h.record(snap(2, "nop"));
    assert_eq!(pcs(&h), Vec::<u32>::new());
    assert!(h.dump().contains("Execution Trace"));
}

#[test]
fn empty_history_iterates_empty() {
    let h = StateHistory::new(4);
    assert_eq!(pcs(&h), Vec::<u32>::new());
}

#[test]
fn dump_lists_instructions_oldest_first() {
    let mut h = StateHistory::new(2);
    h.record(snap(0, "i32_const"));
    h.record(snap(1, "i32_add"));
    let out = h.dump();
    let first = out.find("i32_const").expect("i32_const missing from dump");
    let second = out.find("i32_add").expect("i32_add missing from dump");
    assert!(first < second, "dump is not oldest-first:\n{out}");
}

#[test]
fn dump_renders_metadata() {
    let mut h = StateHistory::new(2);
    h.record(StateSnapshot {
        pc: JumpTarget(7),
        sp: 1,
        fp: 2,
        instruction: "local_get",
        metadata: Some(("idx", 3)),
    });
    let out = h.dump();
    assert!(out.contains("local_get"), "{out}");
    assert!(out.contains("idx=3"), "{out}");
}
