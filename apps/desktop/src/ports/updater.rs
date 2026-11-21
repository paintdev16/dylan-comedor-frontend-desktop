use std::sync::{Arc, atomic::AtomicU64};

/// Actualización disponible y verificada lista para descargarse.
pub trait AvailableUpdate: Send + Sync {
    fn version(&self) -> &str;

    fn download(
        &self,
        downloaded: &AtomicU64,
        total: &AtomicU64,
    ) -> Result<Box<dyn PreparedUpdate>, String>;
}

/// Paquete descargado y verificado listo para entregarse al instalador externo.
pub trait PreparedUpdate: Send {
    fn handoff(self: Box<Self>) -> Result<(), String>;
}

/// Puerto de actualización desacoplado de GPUI y de Veltrix (permite adaptador real o mocks).
pub trait UpdaterPort: Send + Sync {
    fn is_installed(&self) -> bool;

    fn check(&self) -> Result<Option<Arc<dyn AvailableUpdate>>, String>;
}
