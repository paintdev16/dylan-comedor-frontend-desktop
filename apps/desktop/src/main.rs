#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod features;
mod infra;
mod ports;
mod ui;

use app::{AppShell, lifecycle, services::AppServices};
use gpui_kit::{component::TitleBar, *};

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "--check-update") {
        match infra::veltrix::check() {
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
        if let Err(error) = infra::veltrix::prepare_startup() {
            eprintln!("No se pudo inicializar Dylan: {error}");
            std::process::exit(1);
        }
        match infra::instance::claim() {
            Ok(Some(instance)) => instance,
            Ok(None) => return,
            Err(_) => std::process::exit(1),
        }
    };

    application().with_assets(assets::Assets).run(|cx| {
        init(cx);
        cx.set_global(AppServices::production());

        let mut options = TitleBar::window_options();
        options.window_min_size = Some(size(px(720.0), px(460.0)));
        if let Some(titlebar) = &mut options.titlebar {
            titlebar.title = Some("Comedor Dylan".into());
        }

        open_window(options, cx, |window, cx| {
            let shell = cx.new(|cx| AppShell::new(window, cx));
            let shell_for_close = shell.clone();
            window.on_window_should_close(cx, move |_, cx| {
                let can_close = shell_for_close.read(cx).can_close(cx);
                if can_close {
                    lifecycle::shutdown(cx);
                }
                can_close
            });
            shell
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
