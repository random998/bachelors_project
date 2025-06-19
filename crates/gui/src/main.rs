#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    eframe::WebLogger::init(log::LevelFilter::Debug,).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().expect("No window",).document().expect("No document",);
        let canvas = document
            .get_element_by_id("canvas",)
            .expect("Failed to find Canvas Element",)
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Canvas was not a HtmlCanvasElement",);
        let server_url = document
            .get_element_by_id("server-url",)
            .expect("Failed to find server-address element",)
            .inner_html();
        // TODO let config = freezeout_gui::Config {server_url};
        // eframe::WebRunner::new().start(canvas, Default::default(),
        // Box::new(|cc| Ok(Box::new(freezeout_gui::AppFrame::new(config,
        // cc))))).await.except("failed to start eframe")
    },)
}
