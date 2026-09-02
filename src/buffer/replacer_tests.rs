use super::*;

fn drain(replacer: &mut LruReplacer) -> Vec<FrameId> {
    let mut order = Vec::new();
    while let Some(frame) = replacer.evict() {
        order.push(frame);
    }
    order
}

#[test]
fn an_empty_replacer_has_no_victim() {
    let mut replacer = LruReplacer::new(4);
    assert_eq!(replacer.evict(), None);
}

#[test]
fn a_single_candidate_is_its_own_victim() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(2);

    assert_eq!(replacer.evict(), Some(2));
    assert_eq!(replacer.evict(), None);
}

#[test]
fn the_least_recently_inserted_goes_first() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(0);
    replacer.insert(1);
    replacer.insert(2);

    assert_eq!(drain(&mut replacer), vec![0, 1, 2]);
}

#[test]
fn reinserting_moves_a_frame_to_the_front() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(0);
    replacer.insert(1);
    replacer.insert(2);
    replacer.insert(0);

    assert_eq!(drain(&mut replacer), vec![1, 2, 0]);
}

#[test]
fn reinserting_the_only_frame_keeps_the_list_intact() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(3);
    replacer.insert(3);

    assert_eq!(drain(&mut replacer), vec![3]);
}

#[test]
fn removing_the_middle_leaves_the_order_intact() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(0);
    replacer.insert(1);
    replacer.insert(2);

    replacer.remove(1);

    assert_eq!(drain(&mut replacer), vec![0, 2]);
}

#[test]
fn removing_either_end_leaves_the_order_intact() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(0);
    replacer.insert(1);
    replacer.insert(2);

    replacer.remove(0);
    replacer.remove(2);

    assert_eq!(drain(&mut replacer), vec![1]);
}

#[test]
fn removing_an_unlinked_frame_does_nothing() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(0);

    replacer.remove(1);
    replacer.remove(0);
    replacer.remove(0);

    assert_eq!(replacer.evict(), None);
}

#[test]
fn a_frame_can_come_back_after_being_evicted() {
    let mut replacer = LruReplacer::new(4);
    replacer.insert(1);
    assert_eq!(replacer.evict(), Some(1));

    replacer.insert(1);
    replacer.insert(0);

    assert_eq!(drain(&mut replacer), vec![1, 0]);
}
