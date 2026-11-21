use gpui_kit::{
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, h_flex,
        label::Label, tag::Tag,
    },
    prelude::FluentBuilder as _,
    *,
};

/// Barra de título personalizada de Dylan sobre `gpui_kit::component::TitleBar`.
/// No depende de ningún feature: recibe opcionalmente un slot derecho (p. ej. actualizaciones).
pub fn app_title_bar(
    title: impl Into<SharedString>,
    version: &str,
    right_slot: Option<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let primary = cx.theme().primary;

    TitleBar::new()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::LayoutDashboard)
                        .small()
                        .text_color(primary),
                )
                .child(Label::new(title).text_sm().font_semibold())
                .child(
                    Tag::secondary()
                        .small()
                        .rounded_full()
                        .child(format!("v{version}")),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .pr_2()
                .when_some(right_slot, |slot, child| slot.child(child)),
        )
}
