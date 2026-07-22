//! Structs for keeping track where in the DOM a node belongs

use std::cell::RefCell;
use std::marker::PhantomData;

use slab::Slab;
use web_sys::{Element, Node};

type PhantomNotSendNorSync = PhantomData<*const u8>;

/// A position in the list of children of an implicit parent [`Element`].
///
/// This can either be in front of a `DomSlot::at(next_sibling)`, at the end of the list with
/// `DomSlot::at_end()`, or a dynamic position in the list with [`DynamicDomSlot::to_position`].
#[derive(Clone)]
pub(crate) struct DomSlot {
    variant: DomSlotVariant,
}

impl std::fmt::Debug for DomSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with_next_sibling(|n| {
            let formatted_node = match n {
                None => None,
                Some(n) if trap_impl::is_trap(n) => Some("<not yet initialized />".to_string()),
                Some(n) => Some(crate::utils::print_node(n)),
            };
            write!(f, "DomSlot {{ next_sibling: {formatted_node:?} }}")
        })
    }
}

#[derive(Clone)]
enum DomSlotVariant {
    Node(Option<Node>),
    Chained(DynamicDomSlotHandle),
}

struct Link {
    parent: DomSlot,
    /// counts the owner + the number of links in DYNAMIC_SLOTS that refer to this link
    /// does NOT count the number of handles
    ref_count: usize,
    has_owner: bool,
}

impl Link {
    fn new(parent: DomSlot) -> Self {
        Self {
            parent,
            ref_count: 1,
            has_owner: true,
        }
    }

    fn dec_owner(&mut self) -> bool {
        debug_assert!(self.has_owner, "must have an owner");
        self.has_owner = false;
        self.dec_ref()
    }

    fn dec_ref(&mut self) -> bool {
        debug_assert!(self.ref_count > 0, "must have refs");
        self.ref_count -= 1;
        self.ref_count == 0
    }

    fn add_ref(&mut self) {
        debug_assert!(self.ref_count > 0, "no revives");
        self.ref_count += 1;
    }
}

thread_local! {
    static DYNAMIC_SLOTS: RefCell<slab::Slab<Link>> = const { RefCell::new(Slab::new()) };
}

type LinkId = usize; // Dictated by slab

/// A dynamic dom slot can be reassigned. This change is also seen by the [`DomSlot`] from
/// [`Self::to_position`] before the reassignment took place.
pub(crate) struct DynamicDomSlot {
    link: LinkId,
    // The link is tied to this specific thread and can't be accessed elsewhere
    _phantom: PhantomNotSendNorSync,
}

impl std::fmt::Debug for DynamicDomSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.link)
    }
}

#[derive(Clone)]
struct DynamicDomSlotHandle {
    link: LinkId,
    // The link is tied to this specific thread and can't be accessed elsewhere
    _phantom: PhantomNotSendNorSync,
}

mod trap_impl {
    use super::Node;
    #[cfg(debug_assertions)]
    thread_local! {
        // A special marker element that should not be referenced
        static TRAP: Node = gloo::utils::document().create_element("div").unwrap().into();
    }
    /// Get a "trap" node, or None if compiled without debug_assertions
    #[cfg(feature = "hydration")]
    pub fn get_trap_node() -> Option<Node> {
        #[cfg(debug_assertions)]
        {
            TRAP.with(|trap| Some(trap.clone()))
        }
        #[cfg(not(debug_assertions))]
        {
            None
        }
    }
    #[inline]
    pub fn is_trap(node: &Node) -> bool {
        #[cfg(debug_assertions)]
        {
            TRAP.with(|trap| node == trap)
        }
        #[cfg(not(debug_assertions))]
        {
            // When not running with debug_assertions, there is no trap node
            let _ = node;
            false
        }
    }
}

impl DomSlot {
    /// Denotes the position just before the given node in its parent's list of children.
    pub fn at(next_sibling: Node) -> Self {
        Self::create(Some(next_sibling))
    }

    /// Denotes the position at the end of a list of children. The parent is implicit.
    pub fn at_end() -> Self {
        Self::create(None)
    }

    pub fn create(next_sibling: Option<Node>) -> Self {
        Self {
            variant: DomSlotVariant::Node(next_sibling),
        }
    }

    /// A new "placeholder" [DomSlot] that should not be used to insert nodes
    #[inline]
    #[cfg(feature = "hydration")]
    pub fn new_debug_trapped() -> Self {
        Self::create(trap_impl::get_trap_node())
    }

    /// Get the [Node] that comes just after the position, or `None` if this denotes the position at
    /// the end
    fn with_next_sibling_check_trap<R>(&self, f: impl FnOnce(Option<&Node>) -> R) -> R {
        let checkedf = |node: Option<&Node>| {
            assert!(
                node.is_none_or(|node| !trap_impl::is_trap(node)),
                "Should not use a trapped DomSlot. Please report this as an internal bug in yew."
            );
            f(node)
        };
        self.with_next_sibling(checkedf)
    }

    fn with_next_sibling<R>(&self, f: impl FnOnce(Option<&Node>) -> R) -> R {
        match &self.variant {
            DomSlotVariant::Node(n) => f(n.as_ref()),
            DomSlotVariant::Chained(chain) => chain.with_next_sibling(f),
        }
    }

    /// Insert a [Node] at the position denoted by this slot. `parent` must be the actual parent
    /// element of the children that this slot is implicitly a part of.
    pub(super) fn insert(&self, parent: &Element, node: &Node) {
        self.with_next_sibling_check_trap(|next_sibling: Option<&Node>| {
            parent
                .insert_before(node, next_sibling)
                .unwrap_or_else(|err| {
                    let msg = if next_sibling.is_some() {
                        "failed to insert node before next sibling"
                    } else {
                        "failed to append child"
                    };
                    // Log normally, so we can inspect the nodes in console
                    gloo::console::error!(msg, err, parent, next_sibling, node);
                    // Log via tracing for consistency
                    tracing::error!(msg);
                    // Panic to short-circuit and fail
                    panic!("{}", msg)
                });
        });
    }

    #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
    #[cfg(test)]
    fn get(&self) -> Option<Node> {
        self.with_next_sibling(|n| n.cloned())
    }
}

impl DynamicDomSlot {
    /// Create a dynamic dom slot that initially represents ("targets") the same slot as the
    /// argument.
    pub fn new(initial_position: DomSlot) -> Self {
        let link = DYNAMIC_SLOTS.with_borrow_mut(|slots| {
            if let DomSlotVariant::Chained(parent) = &initial_position.variant {
                slots.get_mut(parent.link).unwrap().add_ref();
            }
            slots.insert(Link::new(initial_position))
        });
        Self {
            link,
            _phantom: PhantomData,
        }
    }

    #[cfg(feature = "hydration")]
    pub fn new_debug_trapped() -> Self {
        Self::new(DomSlot::new_debug_trapped())
    }

    /// Move out of self, leaving behind a trapped slot. `self` should not be used afterwards.
    /// Used during the transition from a hydrating to a rendered component to move state between
    /// enum variants.
    #[cfg(feature = "hydration")]
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::new(DomSlot::new_debug_trapped()))
    }

    /// Change the [`DomSlot`] that is targeted. Subsequently, this will behave as if `self` was
    /// created from the passed DomSlot in the first place.
    pub fn reassign(&self, next_position: DomSlot) {
        self.clone_to_follower().reassign_unchecked(next_position);
    }

    /// Get a [`DomSlot`] that gets automatically updated when `self` gets reassigned. All such
    /// slots are equivalent to each other and point to the same position.
    pub fn to_position(&self) -> DomSlot {
        DomSlot {
            variant: DomSlotVariant::Chained(self.clone_to_follower()),
        }
    }

    /// There can only be one owner of a dynamic dom slot. Reassigning a dom slot is only allowed
    /// while that owner is still alive. All other accesses (e.g. through DomSlot) are followers
    /// and should only read the value, but never write to it.
    /// This does not imply that access is always serialized! Followers are allowed to write at any
    /// point without prior synchronization, as long as they ensure that the owner is still alive.
    fn clone_to_follower(&self) -> DynamicDomSlotHandle {
        DynamicDomSlotHandle {
            link: self.link,
            _phantom: self._phantom,
        }
    }
}

fn remove_link(slots: &mut Slab<Link>, link: LinkId) {
    let mut link = link;
    loop {
        let removed = slots.remove(link);
        let DomSlotVariant::Chained(handle) = removed.parent.variant else {
            break;
        };
        if !slots.get_mut(handle.link).unwrap().dec_ref() {
            break;
        }
        link = handle.link;
    }
    // from time to time, clean up memory in the slab
    const ALLOWED_SLACK: usize = 1024 * 1024 * 1024 / size_of::<Link>();
    if slots.capacity() / 4 > slots.len() && slots.capacity() - slots.len() > ALLOWED_SLACK {
        slots.shrink_to_fit();
    }
}

impl Drop for DynamicDomSlot {
    fn drop(&mut self) {
        DYNAMIC_SLOTS.with_borrow_mut(|slots| {
            if slots.get_mut(self.link).unwrap().dec_owner() {
                remove_link(slots, self.link);
            }
        });
    }
}

impl DynamicDomSlotHandle {
    /// Reassign through a handle. This is only valid if the owning [DynamicDomSlot] is still alive.
    fn reassign_unchecked(&self, next_position: DomSlot) {
        // TODO: is not defensive against accidental reference loops
        DYNAMIC_SLOTS.with_borrow_mut(|slots| {
            let old_parent = slots.get_mut(self.link).unwrap().parent.clone();
            match (&old_parent.variant, &next_position.variant) {
                (DomSlotVariant::Node(_), DomSlotVariant::Node(_)) => {}
                (DomSlotVariant::Node(_), DomSlotVariant::Chained(new_parent)) => {
                    slots.get_mut(new_parent.link).unwrap().add_ref();
                }
                (DomSlotVariant::Chained(old_parent), DomSlotVariant::Node(_)) => {
                    if slots.get_mut(old_parent.link).unwrap().dec_ref() {
                        remove_link(slots, old_parent.link);
                    }
                }
                (DomSlotVariant::Chained(old_parent), DomSlotVariant::Chained(new_parent)) => {
                    if old_parent.link == new_parent.link {
                        return;
                    }
                    slots.get_mut(new_parent.link).unwrap().add_ref();
                    if slots.get_mut(old_parent.link).unwrap().dec_ref() {
                        remove_link(slots, old_parent.link);
                    }
                }
            }
            slots.get_mut(self.link).unwrap().parent = next_position;
        });
    }

    fn with_next_sibling<R>(&self, f: impl FnOnce(Option<&Node>) -> R) -> R {
        // We use an iterative approach to traverse a possible long chain of references.
        // See issue #3043 for why a recursive call is impossible for large lists in vdom.
        let node = DYNAMIC_SLOTS.with_borrow(|slots| {
            let mut link = self.link;
            loop {
                match &slots.get(link).unwrap().parent.variant {
                    // NOTE: We clone to drop the borrow and let f re-enter this method
                    DomSlotVariant::Node(node) => break node.clone(),
                    DomSlotVariant::Chained(handle) => link = handle.link,
                }
            }
        });
        f(node.as_ref())
    }
}

#[cfg(feature = "hydration")]
mod feat_hydration {
    use std::marker::PhantomData;

    use web_sys::Node;

    use super::{DomSlot, DynamicDomSlot, DynamicDomSlotHandle};

    pub struct SlotBulletin<'tree> {
        prev_next_sibling: Option<DynamicDomSlotHandle>,
        _owner: PhantomData<&'tree mut DynamicDomSlot>,
    }
    impl<'tree> SlotBulletin<'tree> {
        pub fn start(slot: &'tree mut DynamicDomSlot) -> Self {
            // We take a follower, but we are sure the owner is alive
            Self {
                prev_next_sibling: Some(slot.clone_to_follower()),
                _owner: PhantomData,
            }
        }

        pub fn new() -> Self {
            Self {
                prev_next_sibling: None,
                _owner: PhantomData,
            }
        }

        fn write(&mut self, pos: DomSlot) {
            if let Some(slot) = &mut self.prev_next_sibling {
                slot.reassign_unchecked(pos);
            }
        }

        pub fn write_at_node(&mut self, node: Node) {
            self.write(DomSlot::at(node));
            self.prev_next_sibling = None;
        }

        // This method does not track that `inner_next_sibling` (which is the owner) lives for
        // lifetime of this call. This must be done by the caller, which puts it somewhere in
        // its component state
        pub fn write_at_comp(&mut self, slot: DomSlot, inner_next_sibling: &DynamicDomSlot) {
            self.write(slot);
            self.prev_next_sibling = Some(inner_next_sibling.clone_to_follower());
        }
    }
    impl Drop for SlotBulletin<'_> {
        fn drop(&mut self) {
            self.write(DomSlot::at_end())
        }
    }
}
#[cfg(feature = "hydration")]
pub(crate) use feat_hydration::SlotBulletin;

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
#[cfg(test)]
mod layout_tests {
    use gloo::utils::document;
    use wasm_bindgen_test::{wasm_bindgen_test as test, wasm_bindgen_test_configure};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[test]
    fn new_at_and_get() {
        let node = document().create_element("p").unwrap();
        let position = DomSlot::at(node.clone().into());
        assert_eq!(
            position.get().unwrap(),
            node.clone().into(),
            "expected the DomSlot to be at {node:#?}"
        );
    }

    #[test]
    fn new_at_end_and_get() {
        let position = DomSlot::at_end();
        assert!(
            position.get().is_none(),
            "expected the DomSlot to not have a next sibling"
        );
    }

    #[test]
    fn get_through_dynamic() {
        let original = DomSlot::at(document().create_element("p").unwrap().into());
        let target = DynamicDomSlot::new(original.clone());
        assert_eq!(
            target.to_position().get(),
            original.get(),
            "expected {target:#?} to point to the same position as {original:#?}"
        );
    }

    #[test]
    fn get_after_reassign() {
        let target = DynamicDomSlot::new(DomSlot::at_end());
        let target_pos = target.to_position();
        // We reassign *after* we called `to_position` here to be strict in the test
        let replacement = DomSlot::at(document().create_element("p").unwrap().into());
        target.reassign(replacement.clone());
        assert_eq!(
            target_pos.get(),
            replacement.get(),
            "expected {target:#?} to point to the same position as {replacement:#?}"
        );
    }

    #[test]
    fn get_chain_after_reassign() {
        let middleman = DynamicDomSlot::new(DomSlot::at_end());
        let target = DynamicDomSlot::new(middleman.to_position());
        let target_pos = target.to_position();
        assert!(
            target.to_position().get().is_none(),
            "should not yet point to a node"
        );
        // Now reassign the middle man, but get the node from `target`
        let replacement = DomSlot::at(document().create_element("p").unwrap().into());
        middleman.reassign(replacement.clone());
        assert_eq!(
            target_pos.get(),
            replacement.get(),
            "expected {target:#?} to point to the same position as {replacement:#?}"
        );
    }

    #[test]
    fn debug_printing() {
        // basic tests that these don't panic. We don't enforce any specific format.
        println!("At end: {:?}", DomSlot::at_end());
        println!("Trapped: {:?}", DomSlot::new_debug_trapped());
        println!(
            "At element: {:?}",
            DomSlot::at(document().create_element("p").unwrap().into())
        );
    }
}
