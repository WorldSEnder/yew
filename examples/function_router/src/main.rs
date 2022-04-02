fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_logger::init(wasm_logger::Config::new(log::Level::Trace));
    #[cfg(feature = "csr")]
    yew::Renderer::<function_router::App>::new().render();
}
