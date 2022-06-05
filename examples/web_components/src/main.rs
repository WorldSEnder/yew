use bindings::FromAttributeMap;
use wasm_bindgen::throw_val;
use web_sys::NamedNodeMap;
use yew::prelude::*;

mod bindings;

#[function_component]
fn MyCounter() -> Html {
    let state = use_state(|| 0);

    let incr_counter = {
        let state = state.clone();
        Callback::from(move |_| state.set(*state + 1))
    };

    let decr_counter = {
        let state = state.clone();
        Callback::from(move |_| state.set(*state - 1))
    };

    html!(
        <>
        <button onclick={incr_counter}> {"+"} </button>
        <button onclick={decr_counter}> {"-"} </button>
        <span> {"current count: "} {*state} </span>
        </>
    )
}

#[derive(PartialEq, Properties)]
struct TagProps {
    name: String,
}

impl FromAttributeMap for TagProps {
    fn from_attributes(attrs: &NamedNodeMap) -> Self {
        let name = match attrs.get_named_item("name") {
            Some(name) => name.value(),
            None => "default tag name".into(),
        };
        Self { name }
    }

    fn observed_attribute_names() -> Vec<String> {
        vec!["name".into()]
    }
}

#[function_component]
fn MyTag(props: &TagProps) -> Html {
    html! {
        <>
        <style>{":host { font-family: mono; font-size: 0.8rem; text-decoration: underline; }"}</style>
        <p>{&props.name}</p>
        </>
    }
}

#[function_component]
fn MyDetails() -> Html {
    html! {
        <>
        <style>
        {r#"
        details {font-family: "Open Sans Light",Helvetica,Arial}
        .name {font-weight: bold; color: #217ac0; font-size: 120%}
        h4 { margin: 10px 0 -8px 0; }
        h4 span { background: #217ac0; padding: 2px 6px 2px 6px }
        h4 span { border: 1px solid #cee9f9; border-radius: 4px }
        h4 span { color: white }
        .attributes { margin-left: 22px; font-size: 90% }
        .attributes p { margin-left: 16px; font-style: italic }
        "#}
        </style>
        <details>
          <summary>
            <span>
              <code class="name">{"<"}<slot name="element-name">{"NEEDS A NAME"}</slot>{">"}</code>
              <i class="desc"><slot name="description">{"NEEDS A DESCRIPTION"}</slot></i>
            </span>
          </summary>
          <div class="attributes">
            <h4><span>{"Attributes"}</span></h4>
            <slot name="attributes"><p>{"None"}</p></slot>
          </div>
        </details>
        <hr />
        </>
    }
}

fn main() {
    if let Err(e) = bindings::define::<MyCounter>("my-counter") {
        throw_val(e)
    }
    if let Err(e) = bindings::define::<MyTag>("my-tag") {
        throw_val(e)
    }
    if let Err(e) = bindings::define::<MyDetails>("my-details") {
        throw_val(e)
    }
}
