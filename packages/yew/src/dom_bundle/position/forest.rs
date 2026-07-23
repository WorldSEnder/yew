use slab::Slab;

use super::{DomSlot, DomSlotVariant, Node};

#[derive(Default)]
pub struct LinkForest {
    nodes: Slab<Link>,
}

#[allow(unused)]
macro_rules! trace {
    ($msg:literal $(,)?) => {
        ::gloo::console::log!(
            ::std::format!("%c[{}:{}] ", ::std::file!(), ::std::line!()),
            "font-weight: bold",
            ::std::format!($msg)
        )
    };
    ($msg:literal , $( $args:tt ),*) => {
        ::gloo::console::log!(
            ::std::format!("%c[{}:{}] ", ::std::file!(), ::std::line!()),
            "font-weight: bold",
            ::std::format!($msg, $( $args ),* ),
        )
    }
}

impl LinkForest {
    #[allow(unused)]
    fn print_all(&self) {
        for (n, node) in &self.nodes {
            gloo::console::console_dbg!(node.debug(n));
        }
    }

    pub fn insert(&mut self, link: DomSlot) -> LinkId {
        let entry = self.nodes.vacant_entry();
        let link_id = entry.key();
        let (link, parent) = Link::new(link, link_id);
        entry.insert(link);
        if let Some(parent) = parent {
            self.node_mut(parent).add_ref();
        }
        link_id
    }

    fn node(&self, link: LinkId) -> &Link {
        &self.nodes[link]
    }

    fn node_mut(&mut self, link: LinkId) -> &mut Link {
        &mut self.nodes[link]
    }

    fn remove_node(&mut self, link: LinkId) -> Link {
        self.nodes.remove(link)
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
        let mut n = link;
        loop {
            let node = self.remove_node(n);
            debug_assert!(
                node.right(n).is_none(),
                "can't have children in the represented tree"
            );
            let l = node.left(n);
            let rep_p = node.rep_parent(n);
            let p = node.parent;
            if let &LinkParent::AuxParent(p) = &p {
                debug_assert!(self.node(p).right(p) == Some(n));
                self.node_mut(p).set_right(p, l);
            }
            if let Some(l) = l {
                self.node_mut(l).parent = p;
            }
            let Some(rep_p) = rep_p else {
                break;
            };
            if !self.node_mut(rep_p).dec_ref(false) {
                break;
            }
            n = rep_p;
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
        debug_assert!(
            self.node(link).has_owner(),
            "owner must be alive to reassign"
        );
        let old_parent_id = self.node(link).rep_parent(link);
        let (new_parent, new_parent_id) = match new_parent.variant {
            DomSlotVariant::Chained(link) => (LinkParent::PathParent(link.link), Some(link.link)),
            DomSlotVariant::Node(data) => (LinkParent::Root(data), None),
        };
        match (old_parent_id, new_parent_id) {
            // nothing to do
            (Some(old_parent), Some(new_parent)) if old_parent == new_parent => return,
            _ => {}
        }
        if let Some(new_parent_id) = new_parent_id {
            self.node_mut(new_parent_id).add_ref();
        }
        self.splay(link);
        let l = self.node(link).left(link);
        let parent = std::mem::replace(&mut self.node_mut(link).parent, new_parent);
        self.node_mut(link).set_left(link, None);
        self.node_mut(link).set_rep_parent(link, new_parent_id);
        if let Some(l) = l {
            self.node_mut(l).parent = parent;
        }
        if let Some(old_parent_id) = old_parent_id {
            self.remove_link(old_parent_id, false);
        }
    }

    pub fn find_root(&mut self, link: LinkId) -> &Option<Node> {
        self.access(link);
        match &self.node(link).parent {
            LinkParent::Root(node) => node,
            _ => unreachable!("access method buggy"),
        }
    }

    // Splay operations on the auxiliary tree
    // In fact, none of the splay operations change the refcount, since they do not modify the
    // represented tree.
    fn splay_parent(&self, link: LinkId) -> Result<LinkId, SplayResult> {
        // Due to borrow issues (fixed with polonius?) we can't borrow data here, and do that
        // in the caller with a double match :/
        match &self.node(link).parent {
            &LinkParent::AuxParent(parent) => Ok(parent),
            &LinkParent::PathParent(link) => Err(SplayResult::Link(link)),
            LinkParent::Root(_) => Err(SplayResult::Root()),
        }
    }

    fn rotate(&mut self, x: LinkId, p: LinkId) {
        let is_left = self.node(p).left(p) == Some(x);
        let m;
        if is_left {
            m = self.node(x).right(x);
            self.node_mut(x).set_right(x, Some(p));
            self.node_mut(p).set_left(p, m);
        } else {
            m = self.node(x).left(x);
            self.node_mut(x).set_left(x, Some(p));
            self.node_mut(p).set_right(p, m);
        };
        if let Some(m) = m {
            debug_assert_eq!(self.node_mut(m).parent, LinkParent::AuxParent(x));
            self.node_mut(m).parent = LinkParent::AuxParent(p);
        }
        let g = std::mem::replace(&mut self.node_mut(p).parent, LinkParent::AuxParent(x));
        if let LinkParent::AuxParent(g) = g {
            let p_was_left = self.node(g).left(g) == Some(p);
            if p_was_left {
                self.node_mut(g).set_left(g, Some(x));
            } else {
                debug_assert!(self.node(g).right(g) == Some(p));
                self.node_mut(g).set_right(g, Some(x));
            }
        }
        self.node_mut(x).parent = g;
    }

    fn splay(&mut self, link: LinkId) -> SplayResult {
        let x = link;
        loop {
            let mut p = match self.splay_parent(x) {
                Ok(p) => p,
                Err(done) => return done,
            };
            if let Ok(g) = self.splay_parent(p) {
                // check for zig-zig or zig-zag
                // zig-zig can be implemented by first rotating p and g, followed by x and p
                // zig-zag can be implemented by first rotating x and p, followed by x and g
                let x_is_left = self.node(p).left(p) == Some(x);
                let p_is_left = self.node(g).left(g) == Some(p);
                if x_is_left == p_is_left {
                    self.rotate(p, g);
                } else {
                    self.rotate(x, p);
                    p = g;
                }
            }
            self.rotate(x, p);
        }
    }

    // Link/cut operations
    fn access(&mut self, link: LinkId) {
        // Also does not change any refcounts
        let (mut curr, mut prev) = (link, None);
        loop {
            let link = self.splay(curr);
            // found a path-parent pointer. now we cut this one
            let d = self.node_mut(curr).right(curr);
            self.node_mut(curr).set_right(curr, prev);
            if let Some(prev) = prev {
                debug_assert_eq!(self.node(prev).parent, LinkParent::PathParent(curr));
                self.node_mut(prev).parent = LinkParent::AuxParent(curr);
            }
            if let Some(d) = d {
                debug_assert_eq!(self.node(d).parent, LinkParent::AuxParent(curr));
                self.node_mut(d).parent = LinkParent::PathParent(curr);
            }
            let SplayResult::Link(link) = link else { break };
            (prev, curr) = (Some(curr), link);
        }
        // now link is on the preferred path, so splay it one last time to put it on top.
        let res = self.splay(link);
        debug_assert!(matches!(res, SplayResult::Root()));
    }
}

pub type LinkId = usize;

enum SplayResult {
    Link(LinkId),
    Root(
        // &'a Option<Node>
    ),
}

#[derive(PartialEq, Debug)]
enum LinkParent {
    Root(Option<Node>),
    // parent is on the same preferred path
    AuxParent(LinkId),
    // "path-parent pointer" to some other preferred path
    PathParent(LinkId),
}

struct Link {
    parent: LinkParent,
    // We use a link's own id to signal that it has no right/left child or represented parent
    left_aux: LinkId,
    right_aux: LinkId,
    rep_parent: LinkId,
    /// counts the owner + the number of links in LINK_FOREST that refer to this link.
    /// to save a bit, the owner is counted in the lowest bit, handles are counted in the upper
    /// bits
    ref_count: usize,
}

impl Link {
    pub fn new(parent: DomSlot, this: LinkId) -> (Self, Option<LinkId>) {
        let (parent, link) = match parent.variant {
            DomSlotVariant::Node(node) => (LinkParent::Root(node), None),
            DomSlotVariant::Chained(handle) => {
                (LinkParent::PathParent(handle.link), Some(handle.link))
            }
        };
        let this = Self {
            parent,
            left_aux: this,
            right_aux: this,
            rep_parent: link.unwrap_or(this),
            ref_count: 1,
        };
        (this, link)
    }

    fn debug(&self, this: LinkId) -> impl '_ + std::fmt::Debug {
        #[expect(unused)]
        #[derive(Debug)]
        struct Link<'a> {
            id: LinkId,
            parent: &'a LinkParent,
            left_aux: Option<LinkId>,
            right_aux: Option<LinkId>,
            rep_parent: Option<LinkId>,
            ref_count: usize,
            has_owner: bool,
        }
        let has_owner = self.has_owner();
        Link {
            id: this,
            parent: &self.parent,
            left_aux: self.left(this),
            right_aux: self.right(this),
            rep_parent: self.rep_parent(this),
            ref_count: self.ref_count / 2 + has_owner as usize,
            has_owner,
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

    fn left(&self, this: LinkId) -> Option<LinkId> {
        (self.left_aux != this).then_some(self.left_aux)
    }

    fn right(&self, this: LinkId) -> Option<LinkId> {
        (self.right_aux != this).then_some(self.right_aux)
    }

    fn set_left(&mut self, this: LinkId, left: Option<LinkId>) {
        self.left_aux = left.unwrap_or(this);
    }

    fn set_right(&mut self, this: LinkId, right: Option<LinkId>) {
        self.right_aux = right.unwrap_or(this);
    }

    fn rep_parent(&self, this: LinkId) -> Option<LinkId> {
        (self.rep_parent != this).then_some(self.rep_parent)
    }

    fn set_rep_parent(&mut self, this: LinkId, rep_parent: Option<LinkId>) {
        self.rep_parent = rep_parent.unwrap_or(this);
    }
}
