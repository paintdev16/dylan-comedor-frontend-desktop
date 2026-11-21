use crate::ports::updater::{AvailableUpdate, PreparedUpdate};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Eventos emitidos por `UpdateStore` hacia el shell raíz (`AppShell`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateEvent {
    /// Veltrix confirmó `READY`: la app debe liberar recursos y cerrarse ordenadamente.
    ReadyToQuit,
}

pub struct DownloadJob {
    pub update: Arc<dyn AvailableUpdate>,
    pub downloaded: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

/// Máquina de estados finita del actualizador (elimina estados imposibles con `bool`/`Option`).
pub enum UpdateState {
    Unavailable,
    Idle,
    Checking,
    Available {
        update: Arc<dyn AvailableUpdate>,
        error: Option<String>,
    },
    Downloading {
        update: Arc<dyn AvailableUpdate>,
        downloaded: Arc<AtomicU64>,
        total: Arc<AtomicU64>,
    },
    ReadyToInstall {
        update: Arc<dyn AvailableUpdate>,
        prepared: Box<dyn PreparedUpdate>,
    },
    Installing {
        update: Arc<dyn AvailableUpdate>,
    },
}

impl UpdateState {
    pub fn initial(installed: bool) -> Self {
        if installed {
            Self::Idle
        } else {
            Self::Unavailable
        }
    }

    pub fn is_visible(&self) -> bool {
        matches!(
            self,
            Self::Available { .. }
                | Self::Downloading { .. }
                | Self::ReadyToInstall { .. }
                | Self::Installing { .. }
        )
    }

    pub fn is_downloading(&self) -> bool {
        matches!(self, Self::Downloading { .. })
    }

    pub fn is_installing(&self) -> bool {
        matches!(self, Self::Installing { .. })
    }

    pub fn download_progress(&self) -> Option<(u64, u64, f64)> {
        let Self::Downloading {
            downloaded, total, ..
        } = self
        else {
            return None;
        };
        let bytes = downloaded.load(Ordering::Relaxed);
        let total_bytes = total.load(Ordering::Relaxed);
        let percentage = if total_bytes > 0 {
            (100.0 * bytes as f64 / total_bytes as f64).min(100.0)
        } else {
            0.0
        };
        Some((bytes, total_bytes, percentage))
    }

    pub fn begin_check(&mut self) -> bool {
        if !matches!(self, Self::Idle) {
            return false;
        }
        *self = Self::Checking;
        true
    }

    pub fn finish_check(&mut self, result: Result<Option<Arc<dyn AvailableUpdate>>, String>) {
        if !matches!(self, Self::Checking) {
            return;
        }
        *self = match result {
            Ok(Some(update)) => Self::Available {
                update,
                error: None,
            },
            Ok(None) | Err(_) => Self::Idle,
        };
    }

    pub fn begin_download(&mut self) -> Option<DownloadJob> {
        let current = std::mem::replace(self, Self::Idle);
        let Self::Available { update, .. } = current else {
            *self = current;
            return None;
        };
        let downloaded = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(0));
        *self = Self::Downloading {
            update: update.clone(),
            downloaded: downloaded.clone(),
            total: total.clone(),
        };
        Some(DownloadJob {
            update,
            downloaded,
            total,
        })
    }

    pub fn finish_download(&mut self, result: Result<Box<dyn PreparedUpdate>, String>) {
        let current = std::mem::replace(self, Self::Idle);
        let Self::Downloading { update, .. } = current else {
            *self = current;
            return;
        };
        *self = match result {
            Ok(prepared) => Self::ReadyToInstall { update, prepared },
            Err(error) => Self::Available {
                update,
                error: Some(format!("Error al descargar: {error}")),
            },
        };
    }

    pub fn begin_install(&mut self) -> Option<Box<dyn PreparedUpdate>> {
        let current = std::mem::replace(self, Self::Idle);
        let Self::ReadyToInstall { update, prepared } = current else {
            *self = current;
            return None;
        };
        *self = Self::Installing { update };
        Some(prepared)
    }

    pub fn fail_install(&mut self, error: String) {
        let current = std::mem::replace(self, Self::Idle);
        let Self::Installing { update } = current else {
            *self = current;
            return;
        };
        *self = Self::Available {
            update,
            error: Some(format!("No se pudo preparar la instalación: {error}")),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePrepared;

    impl PreparedUpdate for FakePrepared {
        fn handoff(self: Box<Self>) -> Result<(), String> {
            Ok(())
        }
    }

    struct FakeAvailable {
        version: String,
    }

    impl AvailableUpdate for FakeAvailable {
        fn version(&self) -> &str {
            &self.version
        }

        fn download(
            &self,
            _downloaded: &AtomicU64,
            _total: &AtomicU64,
        ) -> Result<Box<dyn PreparedUpdate>, String> {
            Ok(Box::new(FakePrepared))
        }
    }

    fn fake_update(version: &str) -> Arc<dyn AvailableUpdate> {
        Arc::new(FakeAvailable {
            version: version.into(),
        })
    }

    #[test]
    fn hidden_until_new_version_is_available() {
        let mut unavailable = UpdateState::initial(false);
        assert!(!unavailable.is_visible());
        assert!(!unavailable.begin_check());

        let mut state = UpdateState::initial(true);
        assert!(!state.is_visible());
        assert!(state.download_progress().is_none());

        assert!(state.begin_check());
        assert!(!state.is_visible());

        state.finish_check(Ok(None));
        assert!(matches!(state, UpdateState::Idle));
        assert!(!state.is_visible());

        assert!(state.begin_check());
        state.finish_check(Err("sin red".into()));
        assert!(matches!(state, UpdateState::Idle));
        assert!(!state.is_visible());

        assert!(state.begin_check());
        state.finish_check(Ok(Some(fake_update("0.3.0"))));
        assert!(state.is_visible());
    }

    #[test]
    fn happy_path_flows_from_available_to_downloading_to_ready_to_installing() {
        let mut state = UpdateState::initial(true);
        assert!(state.begin_check());
        assert!(!state.begin_check());

        state.finish_check(Ok(Some(fake_update("0.3.0"))));
        assert!(state.is_visible());
        assert!(!state.begin_check());

        let job = state.begin_download().expect("debe iniciar descarga");
        assert_eq!(job.update.version(), "0.3.0");
        job.downloaded.store(50, Ordering::Relaxed);
        job.total.store(200, Ordering::Relaxed);
        assert_eq!(state.download_progress(), Some((50, 200, 25.0)));
        assert!(state.is_downloading());
        assert!(state.begin_download().is_none());

        state.finish_download(Ok(Box::new(FakePrepared)));
        assert!(state.is_visible());
        assert!(!state.is_downloading());

        let prepared = state.begin_install().expect("debe iniciar instalación");
        assert!(state.is_visible());
        assert!(state.is_installing());
        assert!(prepared.handoff().is_ok());
    }

    #[test]
    fn download_and_install_errors_keep_update_available_for_retry() {
        let mut state = UpdateState::initial(true);
        assert!(state.begin_check());
        state.finish_check(Ok(Some(fake_update("0.3.0"))));

        assert!(state.begin_download().is_some());
        state.finish_download(Err("red caída".into()));
        assert!(state.is_visible());
        match &state {
            UpdateState::Available { update, error } => {
                assert_eq!(update.version(), "0.3.0");
                assert_eq!(error.as_deref(), Some("Error al descargar: red caída"));
            }
            _ => panic!("Se esperaba volver a Available tras error de descarga"),
        }

        assert!(state.begin_download().is_some());
        state.finish_download(Ok(Box::new(FakePrepared)));
        assert!(state.begin_install().is_some());
        state.fail_install("bloqueado".into());

        assert!(state.is_visible());
        assert!(!state.is_installing());
        match &state {
            UpdateState::Available { update, error } => {
                assert_eq!(update.version(), "0.3.0");
                assert_eq!(
                    error.as_deref(),
                    Some("No se pudo preparar la instalación: bloqueado")
                );
            }
            _ => panic!("Se esperaba volver a Available tras error de instalación"),
        }
    }
}
