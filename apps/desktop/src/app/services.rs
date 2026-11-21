use crate::{infra::veltrix::VeltrixUpdater, ports::updater::UpdaterPort};
use gpui_kit::Global;
use std::sync::Arc;

/// Contenedor global de servicios inyectados en el Composition Root.
pub struct AppServices {
    pub updater: Arc<dyn UpdaterPort>,
}

impl Global for AppServices {}

impl AppServices {
    pub fn production() -> Self {
        Self {
            updater: Arc::new(VeltrixUpdater),
        }
    }
}
