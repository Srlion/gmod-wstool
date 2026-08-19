use std::sync::{Arc, LazyLock, atomic::Ordering};

use arc_swap::ArcSwap;

use crate::steam::{callbacks, client};

static STATE: AtomicState = AtomicState::new(State::DISCONNECTED);
static STATE_MESSAGE: LazyLock<ArcSwap<String>> = LazyLock::new(ArcSwap::default);

#[atomic_enum::atomic_enum]
pub enum State {
    CONNECTED,
    DISCONNECTED,
    FAILURE,
}

impl State {
    pub fn color(&self) -> egui::Color32 {
        match self {
            State::CONNECTED => egui::Color32::GREEN,
            State::DISCONNECTED => egui::Color32::YELLOW,
            State::FAILURE => egui::Color32::RED,
        }
    }

    pub fn message(&self) -> Arc<String> {
        STATE_MESSAGE.load_full()
    }
}

fn set_state(state: State, message: String) {
    STATE.store(state, Ordering::Release);
    STATE_MESSAGE.store(Arc::new(message));
}

pub fn get_state() -> State {
    STATE.load(Ordering::Acquire)
}

pub fn init_state_events() {
    callbacks::register(move |_: steamworks::SteamServersConnected| {
        set_state(State::CONNECTED, "Connected".into());
    });

    callbacks::register(move |s: steamworks::SteamServersDisconnected| {
        set_state(State::DISCONNECTED, s.reason.to_string());
    });

    callbacks::register(move |e: steamworks::SteamServerConnectFailure| {
        set_state(State::FAILURE, e.reason.to_string());
    });

    if client().user().logged_on() {
        set_state(State::CONNECTED, "Connected".into());
    } else {
        set_state(
            State::DISCONNECTED,
            "Disconnected from Steam servers".into(),
        );
    }
}
