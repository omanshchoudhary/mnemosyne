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
#[path = "replacer_tests.rs"]
mod tests;
