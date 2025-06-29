#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use eframe::egui::ViewportBuilder;
use poker_gui::gui;

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    eframe::WebLogger::init(log::LevelFilter::Debug,).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window",)
            .document()
            .expect("No document",);
        let canvas = document
            .get_element_by_id("canvas",)
            .expect("Failed to find Canvas Element",)
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Canvas was not a HtmlCanvasElement",);
        let server_url = document
            .get_element_by_id("server-url",)
            .expect("Failed to find server-address element",)
            .inner_html();

        let config = poker_gui::gui::Config { server_url, };
        eframe::WebRunner::new()
            .start(
                canvas,
                Default::default(),
                Box::new(|cc| {
                    Ok(Box::new(poker_gui::gui::AppFrame::new(config, cc,),),)
                },),
            )
            .await
            .expect("failed to start eframe",)
    },)
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<(),> {
    use clap::Parser;

    #[derive(Debug, Parser,)]
    struct Cli {
        /// The server WebSocket url.
        #[arg(long, short, default_value = "ws://127.0.0.1:9871")]
        url:     String,
        /// The configuration storage key.
        #[arg(long, short)]
        storage: Option<String,>,
    }

    env_logger::builder()
        .filter_level(log::LevelFilter::Info,)
        .format_target(false,)
        .format_timestamp_millis()
        .init();

    let init_size = [1024.0, 640.0,];
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder {
            resizable: Some(true,),
            inner_size: Some(egui::vec2(800.0, 500.0,),),
            ..Default::default()
        }
        .with_inner_size(init_size,)
        .with_min_inner_size(init_size,)
        .with_max_inner_size(init_size,)
        .with_title("Cards",),
        ..Default::default()
    };

    let cli = Cli::parse();

    let config = gui::Config {
        server_url: cli.url,
    };

    let app_name = cli
        .storage
        .map_or_else(|| "freezeout".to_string(), |s| format!("freezeout-{s}"),);

    eframe::run_native(
        &app_name,
        native_options,
        Box::new(|cc| Ok(Box::new(gui::AppFrame::new(config, cc,),),),),
    )
}
