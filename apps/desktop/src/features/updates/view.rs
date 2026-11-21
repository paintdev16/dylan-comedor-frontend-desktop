use crate::features::updates::{state::UpdateState, store::UpdateStore};
use gpui_kit::{
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _,
        button::{Button, ButtonVariants},
        h_flex,
        label::Label,
        progress::Progress,
        spinner::Spinner,
        tag::Tag,
    },
    *,
};

/// Vista delgada de actualizaciones diseñada para vivir dentro de `TitleBar`.
/// Observa `UpdateStore` de forma aislada para no re-renderizar `AppShell` en cada tick de progreso.
pub struct UpdateTitleBarView {
    store: Entity<UpdateStore>,
    _subscription: Subscription,
}

impl UpdateTitleBarView {
    pub fn new(store: Entity<UpdateStore>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            _subscription: subscription,
        }
    }
}

impl Render for UpdateTitleBarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;

        match self.store.read(cx).state() {
            UpdateState::Unavailable | UpdateState::Idle | UpdateState::Checking => div().hidden(),

            UpdateState::Available { update, error } => {
                let version = update.version().to_owned();
                let tooltip = error
                    .clone()
                    .unwrap_or_else(|| format!("Nueva versión disponible: Dylan v{version}"));
                let error_icon = error
                    .as_ref()
                    .map(|_| Icon::new(IconName::TriangleAlert).xsmall().text_color(danger));

                h_flex()
                    .items_center()
                    .gap_1p5()
                    .children(error_icon)
                    .child(
                        Tag::primary()
                            .outline()
                            .small()
                            .rounded_full()
                            .child(format!("v{version}")),
                    )
                    .child(
                        Button::new("download")
                            .primary()
                            .xsmall()
                            .icon(IconName::ArrowDown)
                            .label("Descargar")
                            .tooltip(tooltip)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.update(cx, |store, cx| store.start_download(cx));
                            })),
                    )
            }

            UpdateState::Downloading { update, .. } => {
                let (bytes, total, percentage) = self
                    .store
                    .read(cx)
                    .state()
                    .download_progress()
                    .unwrap_or_default();
                let status = if total > 0 {
                    format!("v{} · {percentage:.0}%", update.version())
                } else {
                    format!("v{} · {bytes} B", update.version())
                };
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Label::new(status).text_xs().text_color(muted))
                    .child(
                        div().w(px(96.0)).child(
                            Progress::new("update-download")
                                .xsmall()
                                .value(percentage as f32)
                                .loading(total == 0)
                                .accessibility_label("Progreso de descarga"),
                        ),
                    )
            }

            UpdateState::ReadyToInstall { update, .. } => {
                let version = update.version().to_owned();
                h_flex().items_center().child(
                    Button::new("install")
                        .success()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .label("Instalar actualización")
                        .tooltip(format!("Instalar Dylan v{version} y reiniciar"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.store.update(cx, |store, cx| store.start_install(cx));
                        })),
                )
            }

            UpdateState::Installing { update } => h_flex().items_center().child(
                Tag::info().small().rounded_full().child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(Spinner::new().xsmall())
                        .child(format!("Instalando v{}...", update.version())),
                ),
            ),
        }
    }
}
