use crate::{
    app::services::AppServices,
    features::updates::state::{UpdateEvent, UpdateState},
    ports::updater::UpdaterPort,
};
use gpui_kit::*;
use std::{sync::Arc, time::Duration};

const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Entidad que coordina la detección, descarga y handoff de actualizaciones con Veltrix.
pub struct UpdateStore {
    updater: Arc<dyn UpdaterPort>,
    state: UpdateState,
    _poll_task: Option<Task<()>>,
    _progress_task: Option<Task<()>>,
    _task: Option<Task<()>>,
}

impl EventEmitter<UpdateEvent> for UpdateStore {}

impl UpdateStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updater = cx.global::<AppServices>().updater.clone();
        let installed = updater.is_installed();
        let mut store = Self {
            updater,
            state: UpdateState::initial(installed),
            _poll_task: None,
            _progress_task: None,
            _task: None,
        };
        if installed {
            store.check_for_updates(cx);
            store.start_periodic_checks(cx);
        }
        store
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    pub fn is_installing(&self) -> bool {
        self.state.is_installing()
    }

    pub fn start_download(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.state.begin_download() else {
            return;
        };
        cx.notify();

        self._progress_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let keep_going = this
                    .update(cx, |store, cx| {
                        if store.state.is_downloading() {
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

        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { job.update.download(&job.downloaded, &job.total) })
                .await;
            let _ = this.update(cx, |store, cx| {
                store._progress_task = None;
                store.state.finish_download(result);
                cx.notify();
            });
        }));
    }

    pub fn start_install(&mut self, cx: &mut Context<Self>) {
        let Some(prepared) = self.state.begin_install() else {
            return;
        };
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { prepared.handoff() }).await;
            let _ = this.update(cx, |store, cx| match result {
                Ok(()) => {
                    cx.emit(UpdateEvent::ReadyToQuit);
                }
                Err(error) => {
                    store.state.fail_install(error);
                    cx.notify();
                }
            });
        }));
    }

    fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if !self.state.begin_check() {
            return;
        }
        let updater = self.updater.clone();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { updater.check() }).await;
            let _ = this.update(cx, |store, cx| {
                store.state.finish_check(result);
                if store.state.is_visible() {
                    cx.notify();
                }
            });
        }));
    }

    fn start_periodic_checks(&mut self, cx: &mut Context<Self>) {
        self._poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(UPDATE_CHECK_INTERVAL).await;
                let keep_alive = this
                    .update(cx, |store, cx| {
                        store.check_for_updates(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_alive {
                    break;
                }
            }
        }));
    }
}
