#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod instance;
mod updater;

use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    label::Label,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

struct HelloGpui {
    first_frame_pending: bool,
    update: Option<updater::Update>,
    installer: Option<tempfile::TempDir>,
    status: String,
    downloading: bool,
    checking: bool,
    installing: Arc<AtomicBool>,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    _progress_task: Option<Task<()>>,
    _task: Option<Task<()>>,
    _outcome_task: Option<Task<()>>,
}

impl HelloGpui {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            first_frame_pending: true,
            update: None,
            installer: None,
            status: String::new(),
            downloading: false,
            checking: false,
            installing: Arc::new(AtomicBool::new(false)),
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            _progress_task: None,
            _task: None,
            _outcome_task: None,
        };
        if updater::installed() {
            view.check_updates(false, cx);
        } else {
            view.status = "Instala Dylan para habilitar las actualizaciones.".into();
        }
        if updater::previous_result().is_some() {
            view._outcome_task = Some(cx.spawn(async move |this, cx| {
                for _ in 0..60 {
                    let outcome = cx
                        .background_spawn(async { updater::previous_result() })
                        .await;
                    let message = match outcome {
                        Some(veltrix_core::UpdateOutcome::Succeeded) => {
                            "Veltrix confirmó la instalación."
                        }
                        Some(veltrix_core::UpdateOutcome::Failed) => {
                            "La actualización falló. Consulta events.jsonl."
                        }
                        Some(veltrix_core::UpdateOutcome::InProgress) => {
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                            continue;
                        }
                        None => return,
                    };
                    let _ = this.update(cx, |view, cx| {
                        if view.status.is_empty() {
                            view.status = message.into();
                            cx.notify();
                        }
                    });
                    return;
                }
            }));
        }
        view
    }

    fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.checking
            || self.downloading
            || self.installing.load(Ordering::Acquire)
            || self.installer.is_some()
        {
            return;
        }
        self.checking = true;
        if manual {
            self.status = "Buscando actualizaciones...".into();
        }
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { updater::check() }).await;
            let _ = this.update(cx, |view, cx| {
                view.checking = false;
                match result {
                    Ok(Some(update)) => {
                        view.update = Some(update);
                        view.status.clear();
                    }
                    Ok(None) => {
                        view.update = None;
                        if manual {
                            view.status = "Dylan está actualizado.".into();
                        }
                    }
                    Err(error) => {
                        if manual {
                            view.status = format!("No se pudo buscar: {error}");
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    fn download_update(&mut self, cx: &mut Context<Self>) {
        if self.downloading {
            return;
        }
        let Some(update) = self.update.clone() else {
            return;
        };
        self.downloading = true;
        self.downloaded.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        let downloaded = self.downloaded.clone();
        let total = self.total.clone();
        self._progress_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let keep_going = this
                    .update(cx, |view, cx| {
                        if view.downloading {
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
        self.status = "Descargando y verificando actualización...".into();
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { updater::download(&update, &downloaded, &total) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.downloading = false;
                match result {
                    Ok(path) => {
                        view.installer = Some(path);
                        view.status = "Descarga verificada. Lista para instalar.".into();
                    }
                    Err(error) => {
                        view.status = format!("Error al descargar: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.installer.take() else {
            return;
        };
        self.installing.store(true, Ordering::Release);
        self.status = "Preparando la instalación con Veltrix...".into();
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { updater::handoff(job) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => {
                    // Dylan no mantiene cambios de usuario pendientes en memoria.
                    // Guardar y cerrar cualquier recurso futuro aquí, antes de salir.
                    cx.quit();
                }
                Err(error) => {
                    view.installing.store(false, Ordering::Release);
                    view.status = format!("No se pudo preparar la instalación: {error}");
                    cx.notify();
                }
            });
        }));
    }
}
impl Render for HelloGpui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.first_frame_pending) {
            // Programar desde el primer render, después de crear el estado de la vista.
            window.on_next_frame(|_, cx| {
                if let Err(error) = veltrix_core::confirm_startup() {
                    eprintln!("No se pudo confirmar el inicio de Dylan: {error}");
                    cx.quit();
                    return;
                }
                if let Err(error) = updater::record_window_started() {
                    eprintln!("No se pudo registrar el inicio de Dylan: {error}");
                }
            });
        }
        let info = self
            .update
            .as_ref()
            .map(|update| Label::new(format!("Dylan v{} está disponible.", update.version)));
        let actions = if self.installing.load(Ordering::Acquire) {
            div().flex()
        } else if self.installer.is_some() {
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("later")
                        .label("Más tarde")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update = None;
                            this.installer = None;
                            this.status.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("install")
                        .primary()
                        .label("Instalar actualización")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.install(cx);
                        })),
                )
        } else if self.update.is_some() && !self.downloading {
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("later")
                        .label("Más tarde")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update = None;
                            this.status.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("download")
                        .primary()
                        .label("Descargar actualización")
                        .on_click(cx.listener(|this, _, _, cx| this.download_update(cx))),
                )
        } else {
            div().flex()
        };
        let progress = if self.downloading {
            let bytes = self.downloaded.load(Ordering::Relaxed);
            let total = self.total.load(Ordering::Relaxed);
            let percentage = if total > 0 {
                (100.0 * bytes as f64 / total as f64).min(100.0)
            } else {
                0.0
            };
            let label = if total > 0 {
                format!("{percentage:.0} % · {bytes} de {total} bytes")
            } else {
                format!("Descargados {bytes} bytes")
            };
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .max_w(px(360.0))
                    .child(
                        Progress::new("update-download")
                            .value(percentage as f32)
                            .loading(total == 0)
                            .accessibility_label("Progreso de descarga"),
                    )
                    .child(Label::new(label)),
            )
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_4()
            .p_6()
            .child(Label::new("Comedor Dylan"))
            .child(Label::new(format!("Dylan v{}", env!("CARGO_PKG_VERSION"))))
            .children(info)
            .children(progress)
            .when(!self.status.is_empty(), |element| {
                element.child(Label::new(self.status.clone()))
            })
            .child(actions)
            .when(
                !self.checking
                    && !self.downloading
                    && !self.installing.load(Ordering::Acquire)
                    && self.update.is_none(),
                |element| {
                    element.child(
                        Button::new("check")
                            .label("Buscar actualizaciones")
                            .on_click(cx.listener(|this, _, _, cx| this.check_updates(true, cx))),
                    )
                },
            )
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "--check-update") {
        match updater::check() {
            Ok(Some(update)) => println!("Disponible: {}", update.version),
            Ok(None) => println!("Sin actualizaciones"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(windows)]
    let _instance = {
        if let Err(error) = updater::prepare_startup() {
            eprintln!("No se pudo inicializar Dylan: {error}");
            std::process::exit(1);
        }
        match instance::claim() {
            Ok(Some(instance)) => instance,
            Ok(None) => return,
            Err(_) => std::process::exit(1),
        }
    };
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);
        open_window(WindowOptions::default(), cx, |window, cx| {
            let view = cx.new(HelloGpui::new);
            let installing = view.read(cx).installing.clone();
            window.on_window_should_close(cx, move |_, _| !installing.load(Ordering::Acquire));
            view
        })
        .expect("No se pudo abrir la ventana");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
