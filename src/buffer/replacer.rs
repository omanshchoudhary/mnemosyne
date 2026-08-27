use crate::buffer::FrameId;

struct Node {
    prev: Option<FrameId>,
    next: Option<FrameId>,
    linked: bool,
}

pub struct LruReplacer {
    nodes: Vec<Node>,
    head: Option<FrameId>, // most recently used
    tail: Option<FrameId>, // next victim
}

impl LruReplacer {
    pub fn new(frame_count: usize) -> Self {
        Self {
            nodes: (0..frame_count)
                .map(|_| Node {
                    prev: None,
                    next: None,
                    linked: false,
                })
                .collect(),
            head: None,
            tail: None,
        }
    }

    pub fn insert(&mut self, frame_id: FrameId) {
        self.unlink(frame_id);

        self.nodes[frame_id].prev = None;
        self.nodes[frame_id].next = self.head;

        match self.head {
            Some(old) => self.nodes[old].prev = Some(frame_id),
            None => self.tail = Some(frame_id),
        }
        self.head = Some(frame_id);
        self.nodes[frame_id].linked = true;
    }

    pub fn remove(&mut self, frame_id: FrameId) {
        self.unlink(frame_id);
    }

    pub fn evict(&mut self) -> Option<FrameId> {
        let victim = self.tail?;
        self.unlink(victim);
        Some(victim)
    }

    fn unlink(&mut self, frame_id: FrameId) {
        if !self.nodes[frame_id].linked {
            return;
        }

        let prev = self.nodes[frame_id].prev;
        let next = self.nodes[frame_id].next;

        match prev {
            Some(p) => self.nodes[p].next = next,
            None => self.head = next,
        }
        match next {
            Some(n) => self.nodes[n].prev = prev,
            None => self.tail = prev,
        }

        self.nodes[frame_id].next = None;
        self.nodes[frame_id].prev = None;
        self.nodes[frame_id].linked = false;
    }
}

#[cfg(test)]
mod tests {
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
}
