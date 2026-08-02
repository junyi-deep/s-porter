//! 应用弹窗的统一尺寸约束和滚动容器。

use gpui::*;
use gpui_component::{dialog::Dialog, scroll::ScrollableElement as _};

/// 防止弹窗高度超过可用窗口；普通 child 区域会据此显示纵向滚动条。
pub(in crate::ui) fn responsive_dialog(dialog: Dialog, window: &Window) -> Dialog {
    let viewport = window.viewport_size();
    let top_margin = viewport.height / 10.;
    let max_height = (viewport.height - top_margin - px(32.)).max(px(320.));
    let max_width = (viewport.width - px(48.)).max(px(320.));
    dialog.max_h(max_height).max_w(max_width)
}

/// content-builder 不会自动创建滚动层，复杂表单需要显式的纵向滚动容器。
pub(in crate::ui) fn scrollable_dialog_body(
    id: &'static str,
    body: impl IntoElement,
    window: &Window,
) -> AnyElement {
    let viewport_height = window.viewport_size().height;
    let height = (viewport_height - viewport_height / 10. - px(136.)).max(px(240.));
    div()
        .id(id)
        .relative()
        .w_full()
        .h(height)
        .min_w_0()
        .overflow_y_scrollbar()
        .child(body)
        .into_any_element()
}
