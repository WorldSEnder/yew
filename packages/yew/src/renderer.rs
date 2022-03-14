use std::cell::Cell;
use std::panic::PanicInfo;
use std::rc::Rc;

use web_sys::Element;

use crate::app_handle::AppHandle;
use crate::html::IntoComponent;

thread_local! {
    static PANIC_HOOK_IS_SET: Cell<bool> = Cell::new(false);
}

/// Set a custom panic hook.
/// Unless a panic hook is set through this function, Yew will
/// overwrite any existing panic hook when one of the `start_app*` functions are called.
#[cfg_attr(documenting, doc(cfg(feature = "render")))]
pub fn set_custom_panic_hook(hook: Box<dyn Fn(&PanicInfo<'_>) + Sync + Send + 'static>) {
    std::panic::set_hook(hook);
    PANIC_HOOK_IS_SET.with(|hook_is_set| hook_is_set.set(true));
}

fn set_default_panic_hook() {
    if !PANIC_HOOK_IS_SET.with(|hook_is_set| hook_is_set.replace(true)) {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    }
}

/// The Yew Renderer.
///
/// This is the main entry point of a Yew application.
#[derive(Debug)]
#[cfg_attr(documenting, doc(cfg(feature = "render")))]
#[must_use = "Renderer does nothing unless render() is called."]
pub struct Renderer<ICOMP>
where
    ICOMP: IntoComponent + 'static,
{
    root: Element,
    props: ICOMP::Properties,
}

impl<ICOMP> Default for Renderer<ICOMP>
where
    ICOMP: IntoComponent + 'static,
    ICOMP::Properties: Default,
{
    fn default() -> Self {
        Self::with_props(Default::default())
    }
}

impl<ICOMP> Renderer<ICOMP>
where
    ICOMP: IntoComponent + 'static,
    ICOMP::Properties: Default,
{
    /// Creates a [Renderer] that renders into the document body with default properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a [Renderer] that renders into a custom root with default properties.
    pub fn with_root(root: Element) -> Self {
        Self::with_root_and_props(root, Default::default())
    }
}

impl<ICOMP> Renderer<ICOMP>
where
    ICOMP: IntoComponent + 'static,
{
    /// Creates a [Renderer] that renders into the document body with custom properties.
    pub fn with_props(props: ICOMP::Properties) -> Self {
        Self::with_root_and_props(
            gloo_utils::document()
                .body()
                .expect("no body node found")
                .into(),
            props,
        )
    }

    /// Creates a [Renderer] that renders into a custom root with custom properties.
    pub fn with_root_and_props(root: Element, props: ICOMP::Properties) -> Self {
        Self { root, props }
    }

    /// Renders the application.
    pub fn render(self) -> AppHandle<ICOMP> {
        set_default_panic_hook();
        AppHandle::<ICOMP>::mount_with_props(self.root, Rc::new(self.props))
    }
}

#[cfg_attr(documenting, doc(cfg(feature = "hydration")))]
#[cfg(feature = "hydration")]
mod feat_hydration {
    use super::*;

    impl<ICOMP> Renderer<ICOMP>
    where
        ICOMP: IntoComponent + 'static,
    {
        /// Hydrates the application.
        pub fn hydrate(self) -> AppHandle<ICOMP> {
            set_default_panic_hook();
            AppHandle::<ICOMP>::hydrate_with_props(self.root, Rc::new(self.props))
        }
    }
}
