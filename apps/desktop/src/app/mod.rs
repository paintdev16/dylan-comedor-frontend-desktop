pub mod lifecycle;
pub mod services;

use crate::{
    features::updates::{UpdateEvent, UpdateStore, UpdateTitleBarView},
    ui::title_bar::app_title_bar,
};
use gpui_kit::{
    component::{ActiveTheme as _, StyledExt as _, label::Label, v_flex},
    *,
};

/// Vista raíz del escritorio Dylan: coordina el ciclo de vida, arranque confirmado y navegación.
pub struct AppShell {
    first_frame_pending: bool,
    updates_visible: bool,
    updates_store: Entity<UpdateStore>,
    updates_view: Entity<UpdateTitleBarView>,
    _subscriptions: Vec<Subscription>,
}

impl AppShell {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updates_store = cx.new(UpdateStore::new);
        let updates_visible = updates_store.read(cx).is_visible();
        let updates_view = cx.new(|cx| UpdateTitleBarView::new(updates_store.clone(), cx));
        let observe_updates = cx.observe(&updates_store, |this, store, cx| {
            let visible = store.read(cx).is_visible();
            if this.updates_visible != visible {
                this.updates_visible = visible;
                cx.notify();
            }
        });
        let subscribe_updates = cx.subscribe(&updates_store, |_, _, event, cx| match event {
            UpdateEvent::ReadyToQuit => {
                lifecycle::shutdown(cx);
                cx.quit();
            }
        });
        Self {
            first_frame_pending: true,
            updates_visible,
            updates_store,
            updates_view,
            _subscriptions: vec![observe_updates, subscribe_updates],
        }
    }

    pub fn can_close(&self, cx: &App) -> bool {
        !self.updates_store.read(cx).is_installing()
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.first_frame_pending) {
            lifecycle::confirm_first_frame(window);
        }

        let theme = cx.theme();
        let update_slot = self
            .updates_visible
            .then(|| self.updates_view.clone().into_any_element());

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(app_title_bar(
                "Comedor Dylan",
                env!("CARGO_PKG_VERSION"),
                update_slot,
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_6()
                    .child(Label::new("Comedor Dylan").text_xl().font_semibold())
                    .child(
                        Label::new(format!("Dylan v{}", env!("CARGO_PKG_VERSION")))
                            .text_sm()
                            .text_color(theme.muted_foreground),
                    ),
            )
    }
}
