use js_sys::Function;
use wasm_bindgen::__rt::WasmRefCell;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsValue, UnwrapThrowExt};
use web_sys::{ElementDefinitionOptions, HtmlElement, ShadowRootInit, ShadowRootMode};
use yew::{AppHandle, BaseComponent};

#[wasm_bindgen]
struct ComponentDescriptor {
    create: fn(&HtmlElement) -> u32,
    destroy: fn(&HtmlElement, u32),
}

#[allow(unused)]
#[wasm_bindgen]
impl ComponentDescriptor {
    pub fn connected(&self, me: &HtmlElement) -> u32 {
        (self.create)(me)
    }

    pub fn disconnected(&self, me: &HtmlElement, ptr: u32) {
        (self.destroy)(me, ptr)
    }

    pub fn adopted(&self, me: &HtmlElement, ptr: u32) {}

    #[wasm_bindgen(js_name = attributeChanged)]
    pub fn attribute_changed(&self, me: &HtmlElement, ptr: u32) {}

    #[wasm_bindgen(getter, js_name = observedAttributes)]
    pub fn observed_attributes(&self) -> Box<[JsValue]> {
        vec![].into()
    }
}

impl ComponentDescriptor {
    fn for_comp<COMP: BaseComponent>() -> Self
    where
        COMP::Properties: Default,
    {
        fn create<COMP: BaseComponent>(this: &HtmlElement) -> u32
        where
            COMP::Properties: Default,
        {
            let shadow = this
                .attach_shadow(&ShadowRootInit::new(ShadowRootMode::Open))
                .expect_throw("shadow root supported");
            let root = gloo::utils::document()
                .create_element("div")
                .expect_throw("can create a wrapper div");
            root.set_attribute("style", "display: contents;")
                .expect_throw("can set display style to contents");
            shadow
                .append_child(&root)
                .expect_throw("can add root node to shadow");
            let handle = yew::Renderer::<COMP>::with_root(root).render();

            Box::into_raw(Box::new(WasmRefCell::new(handle))) as u32
        }
        fn destroy<COMP: BaseComponent>(_me: &HtmlElement, handle: u32) {
            let handle = handle as *mut WasmRefCell<AppHandle<COMP>>;
            wasm_bindgen::__rt::assert_not_null(handle);
            let _ = unsafe { (*handle).borrow_mut() }; // ensure no active borrows
            let app_handle = unsafe { Box::from_raw(handle) };
            app_handle.into_inner().destroy();
        }
        ComponentDescriptor {
            create: create::<COMP>,
            destroy: destroy::<COMP>,
        }
    }
}

#[wasm_bindgen(module = "/js/mk_component.js")]
extern "C" {
    fn make_component(desc: ComponentDescriptor) -> Function;
}

pub fn define<COMP: BaseComponent>(name: &str) -> Result<(), JsValue>
where
    COMP::Properties: Default,
{
    let reg = gloo::utils::window().custom_elements();
    let component_constructor = make_component(ComponentDescriptor::for_comp::<COMP>());
    let options = ElementDefinitionOptions::new();
    reg.define_with_options(name, &component_constructor, &options)?;
    Ok(())
}
