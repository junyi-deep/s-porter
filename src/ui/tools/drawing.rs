//! 基于 gpui-drawer 的独立绘图工具页面。

use gpui::*;
use gpui_drawer::{DrawerApp, DrawerOptions};
use std::path::PathBuf;

pub(in crate::ui) struct DrawingToolState {
    drawer: Option<Entity<DrawerApp>>,
    data_dir: PathBuf,
}

impl DrawingToolState {
    pub(in crate::ui) fn new() -> Self {
        let data_dir = crate::storage::drawing_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("s-porter").join("drawings"));
        Self {
            drawer: None,
            data_dir,
        }
    }
}

pub(in crate::ui) fn render(
    state: &mut DrawingToolState,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let data_dir = state.data_dir.clone();
    let drawer = state
        .drawer
        .get_or_insert_with(|| {
            cx.new(|cx| {
                DrawerApp::with_options(
                    DrawerOptions {
                        data_dir: Some(data_dir),
                        compact: false,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        })
        .clone();

    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .child(drawer)
        .into_any_element()
}
