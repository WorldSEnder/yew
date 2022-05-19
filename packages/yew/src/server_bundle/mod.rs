//! Realizing a virtual dom on the server
//!
//! Implements a very minimal version of DOM. I.e. components are run just before the 'rendered'
//! lifecycle and then serialized to a string

use std::collections::VecDeque;
use std::fmt::Arguments;
use std::io::Write;

type Blocker = (Vec<u8>, SsrScope);
pub(crate) struct SsrSink<'w> {
    output: &'w mut dyn Write,
    buffer: Vec<u8>,
    current_blockers: VecDeque<Blocker>,
    queued_blockers: Vec<VecDeque<Blocker>>,
    pub(crate) hydratable: bool,
}

impl<'w> SsrSink<'w> {
    pub(crate) fn new(output: &'w mut dyn Write, hydratable: bool) -> Self {
        Self {
            output,
            buffer: Vec::new(),
            current_blockers: VecDeque::new(),
            queued_blockers: vec![],
            hydratable,
        }
    }

    fn output(&mut self) -> &mut dyn Write {
        if self.current_blockers.is_empty() {
            &mut self.output
        } else {
            &mut self.buffer
        }
    }

    pub(self) fn write_fmt(&mut self, args: Arguments<'_>) {
        self.output().write_fmt(args).unwrap();
    }

    pub(self) fn push_str(&mut self, str: &str) {
        self.output()
            .write_all(str.as_bytes())
            .expect("writing went wrong");
    }

    pub(self) fn push_text(&mut self, text: &str) {
        html_escape::encode_text_to_writer(&text, &mut self.output()).unwrap();
    }

    pub(self) fn push_double_quoted_attr_value(&mut self, attr: &str) {
        html_escape::encode_double_quoted_attribute_to_writer(attr, &mut self.output()).unwrap();
    }

    pub(crate) fn push_suspended(&mut self, scope: SsrScope) {
        let partial_buffer = std::mem::take(&mut self.buffer);
        self.current_blockers.push_back((partial_buffer, scope));
    }

    pub(crate) async fn run_to_completion(mut self) {
        if let Some((part, blocker)) = self.current_blockers.pop_front() {
            self.output.write_all(&part).unwrap();
            blocker.unblock().await;
            if !self.current_blockers.is_empty() {
                let shelved_blockers = std::mem::take(&mut self.current_blockers);
                self.queued_blockers.push(shelved_blockers);
            }

            blocker.render_to_string(&mut self);

            if self.current_blockers.is_empty() {
                if let Some(unshelved_blockers) = self.queued_blockers.pop() {
                    debug_assert!(!unshelved_blockers.is_empty());
                    self.current_blockers = unshelved_blockers;
                }
            }
        }
        let rest = std::mem::take(&mut self.buffer);
        self.output().write_all(&rest).unwrap();
    }
}

use crate::html::{AnyScope, SsrScope};
use crate::virtual_dom::vcomp::*;

impl VComp {
    pub(crate) fn render_to_string(&self, w: &mut SsrSink<'_>, parent_scope: &AnyScope) {
        self.mountable
            .as_ref()
            .pre_render(parent_scope)
            .render_to_string(w)
    }
}

use crate::virtual_dom::vnode::*;

impl VNode {
    // Boxing is needed here, due to: https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
    pub(crate) fn render_to_string<'a>(
        &'a self,
        w: &'a mut SsrSink<'_>,
        parent_scope: &'a AnyScope,
    ) {
        match self {
            VNode::VTag(vtag) => vtag.render_to_string(w, parent_scope),
            VNode::VText(vtext) => vtext.render_to_string(w, parent_scope),
            VNode::VComp(vcomp) => vcomp.render_to_string(w, parent_scope),
            VNode::VList(vlist) => vlist.render_to_string(w, parent_scope),
            // We are pretty safe here as it's not possible to get a web_sys::Node without
            // DOM support in the first place.
            //
            // The only exception would be to use `ServerRenderer` in a browser or wasm32
            // environment with jsdom present.
            VNode::VRef(_) => {
                panic!("VRef is not possible to be rendered in to a string.")
            }
            // Portals are not rendered.
            VNode::VPortal(_) => {}
            VNode::VSuspense(vsuspense) => vsuspense.render_to_string(w, parent_scope),
        }
    }
}

use crate::virtual_dom::vlist::*;

impl VList {
    pub(crate) fn render_to_string(&self, w: &mut SsrSink<'_>, parent_scope: &AnyScope) {
        for child in self.children.iter() {
            child.render_to_string(w, parent_scope)
        }
    }
}

use crate::virtual_dom::vsuspense::*;
use crate::virtual_dom::Collectable;

impl VSuspense {
    pub(crate) fn render_to_string(&self, w: &mut SsrSink<'_>, parent_scope: &AnyScope) {
        let collectable = Collectable::Suspense;

        if w.hydratable {
            collectable.write_open_tag(w);
        }

        // always render children on the server side.
        self.children.render_to_string(w, parent_scope);

        if w.hydratable {
            collectable.write_close_tag(w);
        }
    }
}

use crate::virtual_dom::vtag::*;

// Elements that cannot have any child elements.
static VOID_ELEMENTS: &[&str; 14] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

impl VTag {
    pub(crate) fn render_to_string(&self, w: &mut SsrSink<'_>, parent_scope: &AnyScope) {
        write!(w, "<{}", self.tag());

        let write_attr = |w: &mut SsrSink<'_>, name: &str, val: Option<&str>| {
            write!(w, " {}", name);

            if let Some(m) = val {
                w.push_str("=\"");
                w.push_double_quoted_attr_value(m);
                w.push_str("\"");
            }
        };

        if let VTagInner::Input(_) = self.inner {
            if let Some(m) = self.value() {
                write_attr(w, "value", Some(m));
            }

            if self.checked() {
                write_attr(w, "checked", None);
            }
        }

        self.attributes.with_iter(|iter| {
            for (k, v) in iter {
                write_attr(w, k, Some(v));
            }
        });

        write!(w, ">");

        match self.inner {
            VTagInner::Input(_) => {}
            VTagInner::Textarea { .. } => {
                if let Some(m) = self.value() {
                    VText::new(m.to_owned()).render_to_string(w, parent_scope);
                }

                w.push_str("</textarea>");
            }
            VTagInner::Other {
                ref tag,
                ref children,
                ..
            } => {
                if !VOID_ELEMENTS.contains(&tag.as_ref()) {
                    children.render_to_string(w, parent_scope);

                    write!(w, "</{}>", tag);
                } else {
                    // We don't write children of void elements nor closing tags.
                    debug_assert!(children.is_empty(), "{} cannot have any children!", tag);
                }
            }
        }
    }
}

use crate::virtual_dom::vtext::*;

impl VText {
    pub(crate) fn render_to_string(&self, w: &mut SsrSink<'_>, _parent_scope: &AnyScope) {
        w.push_text(&self.text)
    }
}

impl Collectable {
    pub(crate) fn write_open_tag(&self, w: &mut SsrSink<'_>) {
        w.push_str("<!--");
        w.push_str(self.open_start_mark());

        #[cfg(debug_assertions)]
        match self {
            Self::Component(type_name) => w.push_str(type_name),
            Self::Suspense => {}
        }

        w.push_str(self.end_mark());
        w.push_str("-->");
    }

    pub(crate) fn write_close_tag(&self, w: &mut SsrSink<'_>) {
        w.push_str("<!--");
        w.push_str(self.close_start_mark());

        #[cfg(debug_assertions)]
        match self {
            Self::Component(type_name) => w.push_str(type_name),
            Self::Suspense => {}
        }

        w.push_str(self.end_mark());
        w.push_str("-->");
    }
}
