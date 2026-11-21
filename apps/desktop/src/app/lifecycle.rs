use crate::infra::veltrix;
use gpui_kit::{App, Window};

/// Confirma el arranque del protocolo 2 de Veltrix únicamente tras el primer render GPUI.
pub fn confirm_first_frame(window: &mut Window) {
    window.on_next_frame(|_, cx| {
        if let Err(error) = veltrix_core::confirm_startup() {
            eprintln!("No se pudo confirmar el inicio de Dylan: {error}");
            cx.quit();
            return;
        }
        if let Err(error) = veltrix::record_window_started() {
            eprintln!("No se pudo registrar el inicio de Dylan: {error}");
        }
    });
}

/// Punto único de apagado limpio antes de cerrar la ventana o entregar el control a Veltrix.
pub fn shutdown(_cx: &mut App) {
    // Liberar aquí workers de hardware biométrico, conexiones o estado local futuro.
}
