use slab::Slab;

use super::{DomSlot, DomSlotVariant, Node};

#[derive(Default)]
pub struct LinkForest {
    nodes: Slab<Link>,
}

impl LinkForest {
    pub fn insert(&mut self, link: DomSlot) -> LinkId {
        let link = Link::new(link);
        let parent = link.parent.link();
        let link_id = self.nodes.insert(link);
        if let Some(parent) = parent {
            self.node_mut(parent).add_ref();
        }
        link_id.try_into().unwrap()
    }

    fn node(&self, link: LinkId) -> &Link {
        &self.nodes[link as usize]
    }

    fn node_mut(&mut self, link: LinkId) -> &mut Link {
        &mut self.nodes[link as usize]
    }

    fn remove_node(&mut self, link: LinkId) -> Link {
        self.nodes.remove(link as usize)
    }

    pub fn remove(&mut self, link: LinkId) {
        self.remove_link(link, true);
    }

    pub fn leak(&mut self, link: LinkId) {
        self.node_mut(link).leak();
    }

    fn remove_link(&mut self, link: LinkId, owner: bool) {
        if !self.node_mut(link).dec_ref(owner) {
            return;
        }
        let mut link = link;
        loop {
            let removed = self.remove_node(link);
            let Some(parent) = removed.parent.link() else {
                break;
            };
            if !self.node_mut(parent).dec_ref(false) {
                break;
            }
            link = parent;
        }
        // from time to time, clean up memory in the slab
        // TODO: this needs more analysis under amortized runtime costs and a clever potential
        // definition. shrink_to_fit will first check if there are any vacant slots at "the end". If
        // there are, it will then do a full pass over empty and filled slots. The problem is that
        // "the end" is not easily available from the public API. We know it's somewhere between
        // len() and capacity(), and also past the link(s) we just removed. But we can't check the
        // internal entries.len().
        const ALLOWED_SLACK: usize = 1024 * 1024 * 1024 / size_of::<Link>();
        let slots = &mut self.nodes;
        if slots.capacity() / 4 > slots.len() && slots.capacity() - slots.len() > ALLOWED_SLACK {
            slots.shrink_to_fit();
        }
    }

    pub fn reassign(&mut self, link: LinkId, new_parent: DomSlot) {
        let new_parent = LinkParent::from(new_parent);
        debug_assert!(
            self.node(link).has_owner(),
            "owner must be alive to reassign"
        );
        let old_parent = self.node(link).parent.link();
        match (old_parent, new_parent.link()) {
            (None, None) => {}
            (None, Some(new_parent)) => {
                self.node_mut(new_parent).add_ref();
            }
            (Some(old_parent), None) => {
                self.remove_link(old_parent, false);
            }
            (Some(old_parent), Some(new_parent)) if old_parent == new_parent => return,
            (Some(old_parent), Some(new_parent)) => {
                self.node_mut(new_parent).add_ref();
                self.remove_link(old_parent, false);
            }
        }
        self.node_mut(link).parent = new_parent;
    }

    pub fn find_root(&self, mut link: LinkId) -> &Option<Node> {
        loop {
            match &self.node(link).parent {
                // NOTE: We clone to drop the borrow and let f re-enter this method
                LinkParent::Root(node) => break node,
                &LinkParent::Some(p_link) => {
                    link = p_link;
                }
            }
        }
    }
}

pub type LinkId = usize;

enum LinkParent {
    Root(Option<Node>),
    Some(LinkId),
}

impl From<DomSlot> for LinkParent {
    fn from(value: DomSlot) -> Self {
        match value.variant {
            DomSlotVariant::Node(node) => Self::Root(node),
            DomSlotVariant::Chained(handle) => Self::Some(handle.link),
        }
    }
}

impl LinkParent {
    fn link(&self) -> Option<LinkId> {
        let &Self::Some(parent) = self else {
            return None;
        };
        Some(parent)
    }
}

struct Link {
    parent: LinkParent,
    /// counts the owner + the number of links in LINK_FOREST that refer to this link.
    /// to save a bit, the owner is counted in the lowest bit, handles are counted in the upper
    /// bits
    ref_count: usize,
}

impl Link {
    pub fn new(parent: DomSlot) -> Self {
        Self {
            parent: parent.into(),
            ref_count: 1,
        }
    }

    fn leak(&mut self) {
        self.add_ref();
        self.dec_ref(true);
    }

    fn has_owner(&self) -> bool {
        (self.ref_count & 0b1) != 0
    }

    fn dec_ref(&mut self, owner: bool) -> bool {
        let weight = if owner { 1 } else { 2 };
        debug_assert!(self.ref_count >= weight, "must have refs");
        self.ref_count -= weight;
        self.ref_count == 0
    }

    fn add_ref(&mut self) {
        debug_assert!(self.ref_count > 0, "no revives");
        self.ref_count += 2;
    }
}
