use super::app::{AppView, Page};
use gpui::*;
use gpui_component::{
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
    },
    *,
};

pub(super) fn render(view_state: &AppView, cx: &mut Context<AppView>) -> impl IntoElement {
    let view = cx.entity();
    let menu_item = |label: &'static str, icon: IconName, page: Page| {
        let view = view.clone();
        SidebarMenuItem::new(label)
            .icon(icon)
            .active(view_state.page == page)
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| {
                    this.page = page;
                    cx.notify();
                });
            })
    };

    Sidebar::new("main-sidebar")
        .collapsible(SidebarCollapsible::None)
        .w(relative(1.))
        .header(
            SidebarHeader::new().child(
                v_flex()
                    .child(div().font_semibold().child("S Porter"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("SSH 与开发工具箱"),
                    ),
            ),
        )
        .child(
            SidebarGroup::new("连接").child(SidebarMenu::new().children([
                menu_item("跳板机", IconName::Settings2, Page::JumpHosts),
                menu_item("SSH 连接", IconName::SquareTerminal, Page::Ssh),
                menu_item("端口转发", IconName::Network, Page::Forward),
            ])),
        )
        .child(
            SidebarGroup::new("实用工具").child(SidebarMenu::new().children([
                menu_item("加解密工具", IconName::Settings2, Page::Crypto),
                menu_item("编解码工具", IconName::Inspector, Page::Codec),
                menu_item("格式化工具", IconName::File, Page::Format),
                menu_item("时间工具", IconName::Calendar, Page::Time),
            ])),
        )
}
