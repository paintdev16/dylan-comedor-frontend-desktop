---
name: shared-contracts-patterns
description: >-
  Guía de arquitectura y patrones para compartir código Rust puro entre el backend (`apps/api`)
  y el frontend de escritorio (`apps/desktop`) mediante `crates/shared` en Dylan.
  Usar cuando se diseñen, creen o modifiquen DTOs, contratos HTTP (`serde`), tipos de dominio,
  reglas de validación, códigos de error o rutas compartidas entre `apps/api` y `apps/desktop`.
  No usar para lógica de UI/GPUI (`apps/desktop`) ni para handlers HTTP, base de datos o infraestructura (`apps/api`).
---

# Patrones de Código Compartido (`crates/shared`)

Este skill define qué código debe vivir en `crates/shared` (`shared`) y cómo diseñarlo para que tanto `apps/api` (backend) como `apps/desktop` (frontend GPUI) compartan una única fuente de verdad sin acoplarse entre sí.

## Dirección de dependencias del workspace

```text
apps/desktop (GPUI / Veltrix)  ──┐
                                 ├──►  crates/shared (Rust puro)
apps/api (Backend HTTP / BD)   ──┘
```

- `apps/desktop` y `apps/api` dependen de `shared = { path = "../../crates/shared" }`.
- `crates/shared` **nunca** depende de `apps/desktop` ni de `apps/api`.
- **Dependencias permitidas en `crates/shared`**: solo utilidades puras y livianas (`serde`, `thiserror`, `chrono`/`time` si se necesitan fechas, `uuid` si se usan identificadores, etc.).
- **Dependencias prohibidas en `crates/shared`**: `gpui-kit`, `axum`, `tokio`, `reqwest`, `sqlx`, `diesel`, `windows-sys`, SDKs de hardware biométrico o cualquier librería de UI, red, archivos o base de datos.

## Estructura recomendada en `crates/shared/src/`

Organizar por responsabilidad y crecer por módulo de negocio cuando aumente el tamaño:

```text
crates/shared/src/
├── lib.rs           # Re-exportaciones públicas limpias
├── domain/          # Tipos de dominio puros, newtypes y enums de negocio
│   ├── diner.rs     # Comensal, documento, estado
│   └── attendance.rs# Turnos de comida, registro de marcación
├── dto/             # Contratos Request/Response serializables (serde)
│   ├── diners.rs
│   └── attendance.rs
├── validation.rs    # Reglas de validación puras reutilizadas en cliente y servidor
├── error.rs         # Códigos y payloads de error compartidos (ApiError)
└── endpoints.rs     # Constantes de rutas HTTP (/api/v1/...)
```

## Patrones obligatorios

### 1. Tipos de dominio y *newtypes* con invariantes

- Usar `enum` explícitos para estados y categorías finitas (ej. `MealShift::Breakfast | Lunch | Dinner`, `DinerStatus::Active | Inactive`) en lugar de `String` libres.
- Encapsular identificadores o valores con formato (ej. `DinerId`, `Dni`) cuando ayude a evitar confusiones de parámetros entre cliente y servidor.
- Mantener los tipos derivados de forma uniforme (`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`).

### 2. Contratos DTO explícitos y versionables

- Separar los tipos de entrada (`CreateDinerRequest`, `RegisterAttendanceRequest`) de los de salida (`DinerResponse`, `AttendanceRecordResponse`) y de las entidades internas de base de datos en `apps/api`.
- Usar una convención de serialización consistente en todo `dto/` (por ejemplo `#[serde(rename_all = "snake_case")]` o `camelCase`, sin mezclar estilos).
- Usar `#[serde(default)]` en campos opcionales nuevos para preservar compatibilidad hacia atrás cuando clientes de escritorio antiguos hablen con un backend más nuevo.
- **Nunca** exponer estructuras de tabla SQL, contraseñas, hashes internos sensibles ni plantillas biométricas crudas salvo en contratos explícitamente diseñados para enrolamiento.

### 3. Validaciones puras en un solo lugar

- Toda regla de validación sin I/O (longitud de documento, nombre no vacío, rango horario permitido, formato de código) debe implementarse como función pura en `crates/shared` devolviendo `Result<_, ValidationError>`.
- `apps/desktop` llama a la validación antes de enviar la petición (feedback inmediato en UI).
- `apps/api` llama exactamente a la misma validación antes de escribir en base de datos (seguridad en servidor).

### 4. Errores compartidos sin filtrar detalles internos

- Definir un `enum ApiErrorCode` (estable y serializable) y una estructura `ApiErrorBody { code, message, field: Option<String> }` en `crates/shared`.
- `apps/api` mapea errores internos (SQL, I/O, panics) a `ApiErrorBody` genéricos sin exponer trazas ni consultas SQL.
- `apps/desktop` interpreta `ApiErrorCode` para mostrar mensajes claros en español o resaltar campos específicos del formulario.

### 5. Rutas HTTP centralizadas

- Declarar prefijos y rutas base en `crates/shared/src/endpoints.rs` (ej. `pub const API_PREFIX: &str = "/api/v1";`, `pub const DINERS: &str = "/api/v1/diners";`) para evitar divergencias por typos entre el router de `apps/api` y el cliente HTTP de `apps/desktop`.

### 6. Pruebas unitarias en `crates/shared`

- Toda regla de validación y contrato crítico debe tener pruebas unitarias rápidas en `crates/shared` verificando:
  - Round-trip JSON (`serde_json::to_string` → `from_str`) y compatibilidad de campos opcionales.
  - Casos válidos e inválidos de cada validador puro.

## Checklist de verificación

Antes de dar por terminado un cambio en `crates/shared`:

1. **Pureza de dependencias:** confirmar que `crates/shared/Cargo.toml` no incluye `gpui-kit`, `axum`, `tokio`, `reqwest`, drivers de BD ni SDKs de hardware.
2. **Sin I/O ni estado global:** comprobar que ninguna función de `crates/shared` accede a red, disco, reloj del sistema no inyectado ni variables de entorno.
3. **Compatibilidad entre apps:** verificar que tanto `apps/api` como `apps/desktop` compilan con los cambios:
   ```powershell
   cargo check --workspace --locked
   cargo test -p shared --locked
   cargo clippy --workspace --all-targets --locked -- -D warnings
   ```
