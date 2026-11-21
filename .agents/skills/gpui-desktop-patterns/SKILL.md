---
name: gpui-desktop-patterns
description: >-
  Guía de arquitectura y patrones recomendados para Rust + GPUI (`gpui-kit`) en `apps/desktop` (Dylan).
  Usar cuando se diseñen, creen o refactoricen vistas, manejo de estado, flujos asíncronos,
  integraciones de hardware biométrico, clientes HTTP o ciclo de vida/actualizaciones dentro de `apps/desktop`.
  No aplicar al backend (`apps/api`) ni introducir dependencias de GPUI en `crates/shared`.
---

# Patrones de Arquitectura para Desktop (Rust + GPUI)

Este skill define cómo estructurar y construir código dentro de `apps/desktop` (`dylan`) manteniendo la UI fluida, testeable sin hardware conectado y completamente desacoplada de `apps/api`.

## Alcance y límites del workspace

- **Solo `apps/desktop`**: estos patrones aplican únicamente al crate `dylan`.
- **`apps/api` es independiente**: el backend (Axum) nunca depende de `apps/desktop` ni de `gpui-kit`.
- **`crates/shared` es Rust puro**: contiene solo contratos/DTOs (`serde`), tipos de dominio, errores y validaciones compartidas. Nunca agregar `gpui-kit`, `axum` ni clientes HTTP/BD en `crates/shared`.

## Arquitectura base: Vertical Slices + Puertos y Adaptadores

Organizar `apps/desktop/src/` por módulo de negocio (`features/`) y aislar toda frontera externa (`ports/` + `infra/`):

```text
apps/desktop/src/
├── main.rs            # CLI flags, single-instance, bootstrap y ventana raíz
├── app/               # AppShell, navegación, AppServices (Global), cierre limpio
├── features/          # Vertical slices (cada uno con entity/state + view + events)
│   ├── attendance/    # Marcación de comida por huella
│   ├── diners/        # Comensales y enrolamiento biométrico
│   └── updates/       # Flujo de actualización con Veltrix
├── ports/             # Traits (BiometricScanner, ApiClient, LocalStore)
├── infra/             # Adaptadores reales + mocks (lector huella, HTTP, Veltrix, Windows)
└── ui/                # Componentes reutilizables sobre gpui-kit, tema, layout, toasts
```

## Patrones obligatorios

### 1. Entity–View reactivo (estilo GPUI / Zed)

- Separar el **estado y la coordinación** en entidades (`Entity<T>`) y mantener las vistas (`impl Render`) delgadas.
- Comunicar cambios entre entidades con `EventEmitter<E>`, `cx.subscribe(...)` y `cx.observe(...)`, evitando pasar cadenas profundas de closures o duplicar estado entre vistas.
- Registrar servicios compartidos transversales (cliente API, servicio biométrico, updater, configuración/sesión) como `Global` de GPUI o dentro de un contenedor de servicios inicializado en `app/`.
- Evitar `Arc<Mutex<_>>` para estado de UI: el estado observable pertenece a `Entity<T>`. Usar atómicos o canales únicamente para cruzar datos entre hilos en segundo plano y el hilo principal de GPUI.
- Conservar los `Subscription` y `Task<()>` relevantes dentro de la entidad correspondiente para que se cancelen automáticamente al destruir la vista o modelo (evitar `.detach()` salvo listeners de ciclo de vida global de la app).

### 2. Máquinas de estado finitas con `enum` (FSM)

- Modelar todo flujo con fases excluyentes —**captura/enrolamiento de huella**, **sincronización** y **ciclo de actualización Veltrix**— con un único `enum` en lugar de combinar múltiples `bool` y `Option`.
- Centralizar las transiciones en métodos de la entidad que validan el estado actual, ejecutan la transición y llaman `cx.notify()` (y `cx.emit(...)` cuando corresponda).
- Objetivo: hacer que estados inválidos (por ejemplo, `checking` y `downloading` simultáneos, o `installing` sin paquete descargado) sean irrepresentables en compilación.

### 3. Actor / Background Worker para hardware e I/O

- Toda interacción bloqueante o externa (lector de huellas SDK/USB/FFI, peticiones HTTP a `apps/api`, BD local, disco, Veltrix) debe:
  1. Definirse detrás de un `trait` en `ports/`.
  2. Contar con un adaptador real y un **adaptador mock/simulado** en `infra/` para desarrollar y probar la UI sin dispositivo físico ni servidor activo.
  3. Ejecutarse fuera del hilo principal de GPUI (`cx.background_spawn(...)` o un hilo/actor dedicado del dispositivo comunicado por canales hacia una `Entity` puente).
- Nunca realizar I/O, llamadas bloqueantes ni lógica de negocio dentro de `impl Render::render`.

### 4. Composition Root y ciclo de vida seguro

- Construir e inyectar los adaptadores (reales o mocks) en un único punto de arranque (`main.rs` / `app/`).
- Respetar el protocolo de arranque confirmado de **Veltrix v0.1.4 (protocolo 2)**:
  - Mantener `updater::prepare_startup()` antes de abrir la ventana.
  - Llamar `veltrix_core::confirm_startup()` y `updater::record_window_started()` únicamente una vez, tras inicializar el estado esencial y dentro de `window.on_next_frame(...)` del primer render.
- Centralizar el apagado limpio en `window.on_window_should_close(...)` y antes de `cx.quit()` (incluyendo el `handoff` de actualización): bloquear cierre durante una instalación activa y detener workers / liberar el lector de huellas antes de salir.

## Ejemplos y recetas de código

Para plantillas concretas de `Entity` + `EventEmitter`, `Global`, FSM de updater/biometría, actor de hardware y cierre limpio en `gpui-kit`, consultar [patterns-cookbook.md](./references/patterns-cookbook.md).

## Checklist de verificación

Antes de dar por terminado un cambio en `apps/desktop`, verificar:

1. **Aislamiento del workspace**: confirmar que ni `apps/api/Cargo.toml` ni `crates/shared/Cargo.toml` agregan dependencias de GPUI.
2. **Pureza de `render()`**: comprobar que ningún `impl Render` realiza I/O, bloquea el hilo UI o muta reglas de negocio complejas.
3. **FSM sin estados imposibles**: revisar que flujos multifase usen `enum` en lugar de banderas `bool` solapadas.
4. **Recursos y tareas**: confirmar que `Task<()>` y `Subscription` están vinculados a su entidad y que los recursos de hardware se liberan al cerrar o actualizar.
5. **Compilación y tests**:
   ```powershell
   cargo check --workspace --locked
   cargo test -p dylan --locked
   ```
