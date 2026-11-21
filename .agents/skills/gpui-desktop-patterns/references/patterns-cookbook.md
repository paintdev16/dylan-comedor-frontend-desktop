# Recetario de Patrones GPUI + Rust (`apps/desktop`)

Plantillas de referencia para aplicar los patrones del skill `gpui-desktop-patterns` sobre `gpui-kit` (`0.10.0`).

---

## 1. Entity + `EventEmitter` + vista delgada

Mantener el estado y las reglas en una entidad (`Model`/`Store`) y dejar que la vista solo proyecte estado y delegue acciones:

```rust
use gpui_kit::*;

#[derive(Clone, Debug)]
pub enum AttendanceEvent {
    Registered { diner_name: String },
    Failed { message: String },
}

pub struct AttendanceStore {
    status: AttendanceState,
    _task: Option<Task<()>>,
}

impl EventEmitter<AttendanceEvent> for AttendanceStore {}

impl AttendanceStore {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            status: AttendanceState::Idle,
            _task: None,
        }
    }

    pub fn state(&self) -> &AttendanceState {
        &self.status
    }
}

pub struct AttendanceView {
    store: Entity<AttendanceStore>,
    _subscriptions: Vec<Subscription>,
}

impl AttendanceView {
    pub fn new(store: Entity<AttendanceStore>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        let subscribe = cx.subscribe(&store, |_this, _store, event, cx| {
            if let AttendanceEvent::Registered { .. } = event {
                // Disparar toast o refresco en otro módulo sin acoplar vistas.
                cx.notify();
            }
        });
        Self {
            store,
            _subscriptions: vec![observe, subscribe],
        }
    }
}
```

---

## 2. Servicios compartidos mediante `Global`

Inyectar adaptadores a través de traits en `ports/` para intercambiar implementaciones reales y mocks desde el *Composition Root*:

```rust
use gpui_kit::*;
use std::sync::Arc;

pub trait BiometricScanner: Send + Sync {
    fn start_capture(&self) -> Result<(), String>;
    fn stop(&self);
}

pub struct AppServices {
    pub scanner: Arc<dyn BiometricScanner>,
}

impl Global for AppServices {}

// En el Composition Root (main.rs / app/bootstrap.rs):
// cx.set_global(AppServices { scanner });
//
// Desde cualquier Entity o View:
// let scanner = cx.global::<AppServices>().scanner.clone();
```

---

## 3. Máquina de estados con `enum` (FSM)

Reemplazar múltiples `bool`/`Option` por un único `enum` con estados mutuamente excluyentes:

```rust
use std::sync::{Arc, atomic::AtomicU64};

pub enum UpdateState {
    Unavailable { reason: String },
    Idle { last_message: Option<String> },
    Checking,
    Available { version: String, update: crate::updater::Update },
    Downloading {
        version: String,
        downloaded: Arc<AtomicU64>,
        total: Arc<AtomicU64>,
    },
    ReadyToInstall {
        version: String,
        job: tempfile::TempDir,
    },
    Installing,
    Error { message: String },
}
```

Ejemplo similar para biometría:

```rust
pub enum AttendanceState {
    Idle,
    WaitingFinger,
    Matching,
    Confirmed { diner_name: String },
    Rejected { reason: String },
    DeviceOffline { reason: String },
}
```

Regla: las transiciones solo aceptan el estado origen válido (`match &self.state { ... }`) y llaman `cx.notify()` al cambiar.

---

## 4. Actor / Background Worker para hardware (lector de huellas)

Cuando el SDK del lector es bloqueante o requiere afinidad de hilo, encapsularlo en un worker dedicado comunicado por canales y conectar una tarea ligera en GPUI que reemita los eventos al hilo UI:

```rust
pub enum ScannerCommand {
    StartCapture,
    CancelCapture,
    Shutdown,
}

pub enum ScannerSignal {
    Connected,
    TemplateCaptured(Vec<u8>),
    Error(String),
    Disconnected,
}
```

Flujo recomendado:
1. Un hilo dedicado (`std::thread::Builder::new().name("biometric-worker".into())`) posee el handle del SDK/dispositivo y escucha `ScannerCommand`.
2. Al capturar una huella o detectar desconexión, envía `ScannerSignal` por un canal asíncrono hacia una tarea registrada en una `Entity` puente (`cx.spawn(async move |this, cx| { while let Ok(signal) = rx.recv().await { ... } })`).
3. Al cerrar la app o antes de `updater::handoff`, enviar `ScannerCommand::Shutdown` y esperar/liberar el dispositivo de forma limpia antes de `cx.quit()`.

---

## 5. Arranque confirmado de Veltrix y cierre limpio

Conservar el orden del ciclo de vida en la vista/shell raíz:

```rust
// Dentro del primer render de la vista raíz (AppShell):
if std::mem::take(&mut self.first_frame_pending) {
    window.on_next_frame(|_, cx| {
        if let Err(error) = veltrix_core::confirm_startup() {
            eprintln!("No se pudo confirmar el inicio de Dylan: {error}");
            cx.quit();
            return;
        }
        if let Err(error) = crate::updater::record_window_started() {
            eprintln!("No se pudo registrar el inicio de Dylan: {error}");
        }
    });
}
```

Y en `window.on_window_should_close`:
- Impedir el cierre si el updater está en `UpdateState::Installing`.
- Si el cierre está permitido, ejecutar la rutina centralizada de apagado (detener worker biométrico, vaciar buffers pendientes) antes de cerrar la ventana.
