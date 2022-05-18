use std::io;

use crate::html::{BaseComponent, Scope};
use crate::server_bundle::SsrSink;

/// A Yew Server-side Renderer.
#[cfg_attr(documenting, doc(cfg(feature = "ssr")))]
#[derive(Debug)]
pub struct ServerRenderer<COMP>
where
    COMP: BaseComponent,
{
    props: COMP::Properties,
    hydratable: bool,
}

impl<COMP> Default for ServerRenderer<COMP>
where
    COMP: BaseComponent,
    COMP::Properties: Default,
{
    fn default() -> Self {
        Self::with_props(COMP::Properties::default())
    }
}

impl<COMP> ServerRenderer<COMP>
where
    COMP: BaseComponent,
    COMP::Properties: Default,
{
    /// Creates a [ServerRenderer] with default properties.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<COMP> ServerRenderer<COMP>
where
    COMP: BaseComponent,
{
    /// Creates a [ServerRenderer] with custom properties.
    pub fn with_props(props: COMP::Properties) -> Self {
        Self {
            props,
            hydratable: true,
        }
    }

    /// Sets whether an the rendered result is hydratable.
    ///
    /// Defaults to `true`.
    ///
    /// When this is sets to `true`, the rendered artifact will include additional information
    /// to assist with the hydration process.
    pub fn hydratable(mut self, val: bool) -> Self {
        self.hydratable = val;

        self
    }

    /// Renders Yew Application.
    pub async fn render(self) -> String {
        let mut s = Vec::new();

        self.render_to_string(&mut s).await;

        String::from_utf8(s).unwrap()
    }

    /// Renders Yew Application to a String.
    pub async fn render_to_string(self, w: &mut dyn io::Write) {
        let mut sink = SsrSink::new(w, self.hydratable);
        let scope = Scope::<COMP>::new(None);
        scope
            .pre_render(self.props.into())
            .render_to_string(&mut sink);
        sink.run_to_completion().await;
    }
}
