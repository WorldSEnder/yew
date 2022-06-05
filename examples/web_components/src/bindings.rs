use std::marker::PhantomData;
use std::ops::Deref;

use js_sys::Function;
use wasm_bindgen::__rt::WasmRefCell;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsValue, UnwrapThrowExt};
use web_sys::{
    Element, ElementDefinitionOptions, HtmlElement, NamedNodeMap, ShadowRootInit, ShadowRootMode,
};
use yew::{html, AppHandle, BaseComponent, Component, Properties};

struct CustomElement<COMP: BaseComponent>(NamedNodeMap, PhantomData<COMP>);
#[derive(PartialEq, Properties)]
struct CustomElementProps {
    attr_map: NamedNodeMap,
}

impl<COMP: BaseComponent> Component for CustomElement<COMP>
where
    COMP::Properties: FromAttributeMap,
{
    type Message = NamedNodeMap;
    type Properties = CustomElementProps;

    fn create(ctx: &yew::Context<Self>) -> Self {
        Self(ctx.props().attr_map.clone(), PhantomData)
    }

    fn view(&self, _: &yew::Context<Self>) -> yew::Html {
        let props = COMP::Properties::from_attributes(&self.0);
        html! {
            <COMP ..props />
        }
    }

    fn update(&mut self, _: &yew::Context<Self>, msg: Self::Message) -> bool {
        self.0 = msg;
        true
    }
}

struct ComponentHandle<COMP: BaseComponent>(Box<WasmRefCell<AppHandle<CustomElement<COMP>>>>)
where
    COMP::Properties: FromAttributeMap;

impl<COMP: BaseComponent> ComponentHandle<COMP>
where
    COMP::Properties: FromAttributeMap,
{
    fn new_in(root: Element, attr_map: NamedNodeMap) -> Self {
        let props = CustomElementProps { attr_map };
        let handle =
            yew::Renderer::<CustomElement<COMP>>::with_root_and_props(root, props).render();

        Self(Box::new(WasmRefCell::new(handle)))
    }

    fn into_abi(self) -> u32 {
        Box::into_raw(self.0) as u32
    }

    unsafe fn from_abi(abi: u32) -> Self {
        let handle = abi as *mut WasmRefCell<AppHandle<CustomElement<COMP>>>;
        wasm_bindgen::__rt::assert_not_null(handle);
        let _ = (*handle).borrow_mut(); // ensure no active borrows
        let app_handle = Box::from_raw(handle);
        Self(app_handle)
    }

    fn destroy(self) {
        self.0.into_inner().destroy()
    }

    unsafe fn ref_from_abi(abi: u32) -> impl Deref<Target = AppHandle<CustomElement<COMP>>> {
        let handle = abi as *mut WasmRefCell<AppHandle<CustomElement<COMP>>>;
        wasm_bindgen::__rt::assert_not_null(handle);
        (*handle).borrow()
    }
}

#[wasm_bindgen]
struct ComponentDescriptor {
    create: fn(&HtmlElement) -> u32,
    destroy: fn(&HtmlElement, u32),
    update: fn(&HtmlElement, u32),
    observed_attrs_as_js: Vec<JsValue>,
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
    pub fn attribute_changed(&self, me: &HtmlElement, ptr: u32) {
        (self.update)(me, ptr)
    }

    #[wasm_bindgen(getter, js_name = observedAttributes)]
    pub fn observed_attributes(&self) -> Box<[JsValue]> {
        self.observed_attrs_as_js.clone().into()
    }
}

impl ComponentDescriptor {
    fn for_comp<COMP: BaseComponent>() -> Self
    where
        COMP::Properties: FromAttributeMap,
    {
        fn create<COMP: BaseComponent>(this: &HtmlElement) -> u32
        where
            COMP::Properties: FromAttributeMap,
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

            ComponentHandle::<COMP>::new_in(root, this.attributes()).into_abi()
        }

        fn destroy<COMP: BaseComponent>(_me: &HtmlElement, handle: u32)
        where
            COMP::Properties: FromAttributeMap,
        {
            let handle = unsafe { ComponentHandle::<COMP>::from_abi(handle) };
            handle.destroy();
        }

        fn update<COMP: BaseComponent>(me: &HtmlElement, handle: u32)
        where
            COMP::Properties: FromAttributeMap,
        {
            let handle = unsafe { ComponentHandle::<COMP>::ref_from_abi(handle) };
            handle.send_message(me.attributes());
        }

        let observed_attrs_as_js = COMP::Properties::observed_attribute_names()
            .iter()
            .map(|s| JsValue::from_str(s))
            .collect();

        ComponentDescriptor {
            create: create::<COMP>,
            destroy: destroy::<COMP>,
            update: update::<COMP>,
            observed_attrs_as_js,
        }
    }
}

#[wasm_bindgen(module = "/js/mk_component.js")]
extern "C" {
    fn make_component(desc: ComponentDescriptor) -> Function;
}

pub fn define<COMP: BaseComponent>(name: &str) -> Result<(), JsValue>
where
    COMP::Properties: FromAttributeMap,
{
    let reg = gloo::utils::window().custom_elements();
    let component_constructor = make_component(ComponentDescriptor::for_comp::<COMP>());
    let options = ElementDefinitionOptions::new();
    reg.define_with_options(name, &component_constructor, &options)?;
    Ok(())
}

pub trait FromAttributeMap {
    fn from_attributes(attrs: &NamedNodeMap) -> Self;
    fn observed_attribute_names() -> Vec<String>;
}

impl FromAttributeMap for () {
    fn from_attributes(_attrs: &NamedNodeMap) -> Self {}

    fn observed_attribute_names() -> Vec<String> {
        vec![]
    }
}
