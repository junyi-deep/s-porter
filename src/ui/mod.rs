mod app;
mod forward_page;
mod sidebar;
mod time_page;
mod tool_page;

use std::borrow::Cow;

use app::AppView;
use gpui::*;
use gpui_component::{Root, TitleBar};
use gpui_component_assets::Assets;

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/trash-2.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/trash-2.svg"))));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = Assets.list(path)?;
        if "icons/trash-2.svg".starts_with(path) {
            assets.push("icons/trash-2.svg".into());
        }
        Ok(assets)
    }
}

pub fn run() {
    let app = gpui_platform::application()
        .with_assets(AppAssets)
        .with_quit_mode(QuitMode::LastWindowClosed);
    app.run(move |cx| {
        gpui_component::init(cx);
        let window_options = WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
            ..Default::default()
        };
        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| AppView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("无法打开 S Porter 主窗口");
        })
        .detach();
    });
}
