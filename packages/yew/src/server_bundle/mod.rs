//! Realizing a virtual dom on the server
//!
//! Implements a very minimal version of DOM. I.e. components are run just before the 'rendered'
//! lifecycle and then serialized to a string

use std::fmt::{self, Arguments, Write};

pub(crate) struct SsrSink<'w> {
    output: &'w mut dyn Write,
    buffer: String,
}

impl<'w> SsrSink<'w> {
    pub(crate) fn new(output: &'w mut dyn Write) -> Self {
        Self {
            output,
            buffer: String::new(),
        }
    }

    pub(self) fn write_fmt(&mut self, args: Arguments<'_>) -> fmt::Result {
        self.buffer.write_fmt(args)
    }

    pub(self) fn push_str(&mut self, str: &str) {
        self.buffer.push_str(str); // .expect("writing went wrong")
    }

    pub(self) fn push_text(&mut self, text: &str) {
        html_escape::encode_text_to_string(&text, &mut self.buffer);
    }

    pub(self) fn push_double_quoted_attr_value(&mut self, attr: &str) {
        html_escape::encode_double_quoted_attribute_to_string(attr, &mut self.buffer);
    }

    pub(crate) async fn run_to_completion(self) {
        self.output.write_str(&self.buffer).unwrap()
    }
}

use crate::html::AnyScope;
use crate::virtual_dom::vcomp::*;

impl VComp {
    pub(crate) async fn render_to_string(
        &self,
        w: &mut SsrSink<'_>,
        parent_scope: &AnyScope,
        hydratable: bool,
    ) {
        self.mountable
            .as_ref()
            .pre_render(parent_scope)
            .render_to_string(w, hydratable)
            .await;
    }
}

use futures::future::{FutureExt, LocalBoxFuture};

use crate::virtual_dom::vnode::*;

impl VNode {
    // Boxing is needed here, due to: https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
    pub(crate) fn render_to_string<'a>(
        &'a self,
        w: &'a mut SsrSink<'_>,
        parent_scope: &'a AnyScope,
        hydratable: bool,
    ) -> LocalBoxFuture<'a, ()> {
        async move {
            match self {
                VNode::VTag(vtag) => vtag.render_to_string(w, parent_scope, hydratable).await,
                VNode::VText(vtext) => vtext.render_to_string(w, parent_scope, hydratable).await,
                VNode::VComp(vcomp) => vcomp.render_to_string(w, parent_scope, hydratable).await,
                VNode::VList(vlist) => vlist.render_to_string(w, parent_scope, hydratable).await,
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
                VNode::VSuspense(vsuspense) => {
                    vsuspense
                        .render_to_string(w, parent_scope, hydratable)
                        .await
                }
            }
        }
        .boxed_local()
    }
}

use crate::virtual_dom::vlist::*;

impl VList {
    pub(crate) async fn render_to_string(
        &self,
        w: &mut SsrSink<'_>,
        parent_scope: &AnyScope,
        hydratable: bool,
    ) {
        for child in self.children.iter() {
            child.render_to_string(w, parent_scope, hydratable).await
        }
    }
}

use crate::virtual_dom::vsuspense::*;
use crate::virtual_dom::Collectable;

impl VSuspense {
    pub(crate) async fn render_to_string(
        &self,
        w: &mut SsrSink<'_>,
        parent_scope: &AnyScope,
        hydratable: bool,
    ) {
        let collectable = Collectable::Suspense;

        if hydratable {
            collectable.write_open_tag(w);
        }

        // always render children on the server side.
        self.children
            .render_to_string(w, parent_scope, hydratable)
            .await;

        if hydratable {
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
    pub(crate) async fn render_to_string(
        &self,
        w: &mut SsrSink<'_>,
        parent_scope: &AnyScope,
        hydratable: bool,
    ) {
        write!(w, "<{}", self.tag()).unwrap();

        let write_attr = |w: &mut SsrSink<'_>, name: &str, val: Option<&str>| {
            write!(w, " {}", name).unwrap();

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

        for (k, v) in self.attributes.iter() {
            write_attr(w, k, Some(v));
        }

        write!(w, ">").unwrap();

        match self.inner {
            VTagInner::Input(_) => {}
            VTagInner::Textarea { .. } => {
                if let Some(m) = self.value() {
                    VText::new(m.to_owned())
                        .render_to_string(w, parent_scope, hydratable)
                        .await;
                }

                w.push_str("</textarea>");
            }
            VTagInner::Other {
                ref tag,
                ref children,
                ..
            } => {
                if !VOID_ELEMENTS.contains(&tag.as_ref()) {
                    children.render_to_string(w, parent_scope, hydratable).await;

                    write!(w, "</{}>", tag).unwrap();
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
    pub(crate) async fn render_to_string(
        &self,
        w: &mut SsrSink<'_>,
        _parent_scope: &AnyScope,
        _hydratable: bool,
    ) {
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
