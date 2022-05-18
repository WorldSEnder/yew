//! This module contains the implementation of a virtual text node `VText`.

use std::cmp::PartialEq;

use super::AttrValue;

/// A type for a virtual
/// [`TextNode`](https://developer.mozilla.org/en-US/docs/Web/API/Document/createTextNode)
/// representation.
#[derive(Clone)]
pub struct VText {
    /// Contains a text of the node.
    pub text: AttrValue,
}

impl VText {
    /// Creates new virtual text node with a content.
    pub fn new(text: impl Into<AttrValue>) -> Self {
        VText { text: text.into() }
    }
}

impl std::fmt::Debug for VText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VText {{ text: \"{}\" }}", self.text)
    }
}

impl PartialEq for VText {
    fn eq(&self, other: &VText) -> bool {
        self.text == other.text
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod ssr_tests {
    use tokio::test;

    use crate::prelude::*;
    use crate::ServerRenderer;

    #[test]
    async fn test_simple_str() {
        #[function_component]
        fn Comp() -> Html {
            html! { "abc" }
        }

        let s = ServerRenderer::<Comp>::new()
            .hydratable(false)
            .render()
            .await;

        assert_eq!(s, r#"abc"#);
    }
}
