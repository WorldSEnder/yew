use std::ops::{Deref, DerefMut};

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
            let node = LinkRef::new(n, node);
            gloo::console::console_dbg!(node.debug());
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

    fn node(&self, link: LinkId) -> LinkRef<&Link> {
        LinkRef::new(link, &self.nodes[link])
    }

    fn node_mut(&mut self, link: LinkId) -> LinkRef<&mut Link> {
        LinkRef::new(link, &mut self.nodes[link])
    }

    fn remove_node(&mut self, link: LinkId) -> LinkRef<Link> {
        LinkRef::new(link, self.nodes.remove(link))
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
                node.right().is_none(),
                "can't have children in the represented tree"
            );
            let l = node.left();
            let rep_p = node.rep_parent();
            let p = node.into_inner().parent;
            if let &LinkParent::AuxParent(p) = &p {
                debug_assert!(self.node(p).right() == Some(n));
                self.node_mut(p).set_right(l);
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
        const ALLOWED_SLACK: usize = 64 * 1024 * 1024 / size_of::<Link>();
        let slots = &mut self.nodes;
        if slots.capacity() / 4 > slots.len() && slots.capacity() - slots.len() > ALLOWED_SLACK {
            slots.shrink_to_fit();
        }
    }

    pub fn reassign(&mut self, link: LinkId, new_parent: DomSlot) {
        // removes `link` from its represented tree and moves it to `new_parent`.
        // we also have to keep track of ref counts.
        debug_assert!(
            self.node(link).has_owner(),
            "owner must be alive to reassign"
        );
        let old_parent_id = self.node(link).rep_parent();
        let (new_parent, new_parent_id) = match new_parent.variant {
            DomSlotVariant::Chained(link) => (LinkParent::PathParent(link.link), Some(link.link)),
            DomSlotVariant::Node(data) => (LinkParent::Root(data), None),
        };
        if old_parent_id == new_parent_id && old_parent_id.is_some() {
            // reassigned to its existing parent, no need to modify.
            return;
        }
        if let Some(new_parent_id) = new_parent_id {
            self.node_mut(new_parent_id).add_ref();
        }
        self.splay(link);
        let l = self.node(link).left();
        let parent = self.node_mut(link).parent.replace(new_parent);
        self.node_mut(link).set_left(None);
        self.node_mut(link).set_rep_parent(new_parent_id);
        if let Some(l) = l {
            self.node_mut(l).parent = parent;
        }
        if let Some(old_parent_id) = old_parent_id {
            self.remove_link(old_parent_id, false);
        }
    }

    pub fn find_root(&mut self, link: LinkId) -> &Option<Node> {
        self.access(link);
        match &self.node(link).into_inner().parent {
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
        // shift the middle node `m` from `x` to `p`.
        let m;
        debug_assert!(self.node(p).right() == Some(x) || self.node(p).left() == Some(x));
        if self.node(p).left() == Some(x) {
            m = self.node(x).right();
            self.node_mut(x).set_right(Some(p));
            self.node_mut(p).set_left(m);
        } else {
            m = self.node(x).left();
            self.node_mut(x).set_left(Some(p));
            self.node_mut(p).set_right(m);
        };
        if let Some(m) = m {
            debug_assert_eq!(self.node_mut(m).parent, LinkParent::AuxParent(x));
            self.node_mut(m).parent = LinkParent::AuxParent(p);
        }
        // attach `x` to the parent of `p`
        let g = self.node_mut(p).parent.replace(LinkParent::AuxParent(x));
        if let LinkParent::AuxParent(g) = g {
            let mut g = self.node_mut(g);
            debug_assert!(g.right() == Some(p) || g.left() == Some(p));
            if g.left() == Some(p) {
                g.set_left(Some(x));
            } else {
                g.set_right(Some(x));
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
                let x_is_left = self.node(p).left() == Some(x);
                let p_is_left = self.node(g).left() == Some(p);
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
            let d = self.node_mut(curr).right();
            if let Some(prev) = prev {
                debug_assert_eq!(self.node(prev).parent, LinkParent::PathParent(curr));
                self.node_mut(prev).parent = LinkParent::AuxParent(curr);
                // small deviation from the original paper: we do not remove the tail
                // of the preferred path the first node is already on.
                // this would originally run unconditionally of prev.is_some()
                self.node_mut(curr).set_right(Some(prev));
                if let Some(d) = d {
                    debug_assert_eq!(self.node(d).parent, LinkParent::AuxParent(curr));
                    self.node_mut(d).parent = LinkParent::PathParent(curr);
                }
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

impl LinkParent {
    fn replace(&mut self, next: LinkParent) -> LinkParent {
        std::mem::replace(self, next)
    }
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

impl AsRef<Link> for Link {
    fn as_ref(&self) -> &Link {
        self
    }
}

impl AsMut<Link> for Link {
    fn as_mut(&mut self) -> &mut Link {
        self
    }
}

struct LinkRef<L> {
    id: LinkId,
    link: L,
}

impl<L: AsRef<Link>> Deref for LinkRef<L> {
    type Target = Link;

    fn deref(&self) -> &Self::Target {
        self.link.as_ref()
    }
}

impl<L: AsRef<Link> + AsMut<Link>> DerefMut for LinkRef<L> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.link.as_mut()
    }
}

impl<L> LinkRef<L> {
    fn new(id: LinkId, link: L) -> Self {
        Self { id, link }
    }

    fn into_inner(self) -> L {
        self.link
    }
}

impl<L: AsRef<Link>> LinkRef<L> {
    fn debug(&self) -> impl '_ + std::fmt::Debug {
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
            id: self.id,
            parent: &self.parent,
            left_aux: self.left(),
            right_aux: self.right(),
            rep_parent: self.rep_parent(),
            ref_count: self.ref_count / 2 + has_owner as usize,
            has_owner,
        }
    }

    fn left(&self) -> Option<LinkId> {
        (self.left_aux != self.id).then_some(self.left_aux)
    }

    fn right(&self) -> Option<LinkId> {
        (self.right_aux != self.id).then_some(self.right_aux)
    }

    fn rep_parent(&self) -> Option<LinkId> {
        (self.rep_parent != self.id).then_some(self.rep_parent)
    }
}

impl<L: AsMut<Link>> LinkRef<L> {
    fn set_left(&mut self, left: Option<LinkId>) {
        self.link.as_mut().left_aux = left.unwrap_or(self.id);
    }

    fn set_right(&mut self, right: Option<LinkId>) {
        self.link.as_mut().right_aux = right.unwrap_or(self.id);
    }

    fn set_rep_parent(&mut self, rep_parent: Option<LinkId>) {
        self.link.as_mut().rep_parent = rep_parent.unwrap_or(self.id);
    }
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
