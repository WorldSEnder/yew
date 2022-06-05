use wasm_bindgen::throw_val;
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

fn main() {
    if let Err(e) = bindings::define::<MyCounter>("my-counter") {
        throw_val(e)
    }
}
