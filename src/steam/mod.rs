use egui::{ColorImage, TextureHandle};
use std::sync::OnceLock;
use steamworks::Client;

pub mod callbacks;
pub mod download;
pub mod items;
pub mod pack;
mod state;
pub mod update;

use crate::ctx;
pub use state::get_state;

static CLIENT: OnceLock<Client> = OnceLock::new();

pub fn client() -> &'static Client {
    CLIENT.get().expect("Steam client not initialized")
}

pub fn init_client() -> Result<(), String> {
    if CLIENT.get().is_none() {
        let client = Client::init_app(4000).map_err(|e| e.to_string())?;
        let _ = CLIENT.set(client);
        state::init_state_events();
    }
    Ok(())
}

pub struct NameAvatar {
    pub name: String,
    pub avatar: TextureHandle,
}

pub fn name_avatar() -> NameAvatar {
    let steam_id = client().user().steam_id();
    let friends = client().friends();
    let user = friends.get_friend(steam_id);

    let avatar = user
        .large_avatar()
        .map(|pixels| {
            // Steam large avatars are 184x184, RGBA format
            let size = 184;
            ColorImage::from_rgba_unmultiplied([size, size], &pixels)
        })
        .unwrap_or_else(ColorImage::example);

    let avatar = ctx().load_texture("steam_avatar", avatar, egui::TextureOptions::LINEAR);

    NameAvatar {
        name: friends.name(),
        avatar,
    }
}
