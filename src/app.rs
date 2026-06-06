use eframe::egui;
use egui::{Spinner, Vec2};

use crate::async_load::AsyncLoad;
use crate::ignores;
use crate::tabs::Tab;
use crate::tabs::download::DownloadPanel;
use crate::tabs::my_workshop::WorkshopPanel;
use crate::ui;
use crate::whitelist;
use crate::{settings, steam};

const NAV_HEIGHT: f32 = 72.0;
const TAB_BLOCK_W: f32 = 248.0; // 2 * 120 + 8 gap
const FOOTER_W: f32 = 150.0;

const BABY_BLUE: egui::Color32 = egui::Color32::from_rgb(137, 207, 240);
const GOLD: egui::Color32 = egui::Color32::from_rgb(255, 200, 60);

pub struct App {
    current_tab: Tab,
    loaded: AsyncLoad<Result<(), String>>,
    username_avatar: AsyncLoad<steam::NameAvatar>,
    workshop_panel: WorkshopPanel,
    download_panel: DownloadPanel,
    whitelist_refresh: AsyncLoad<bool>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let dest = cc
            .storage
            .map(|s| settings::load_download_path(s))
            .unwrap_or_else(settings::default_download_path);
        if let Some(s) = cc.storage {
            whitelist::seed(s.get_string(whitelist::STORAGE_KEY));
            ignores::seed(s.get_string(ignores::STORAGE_KEY));
        }
        Self {
            current_tab: Tab::MyWorkshop,
            loaded: AsyncLoad::new(|_| steam::init_client()),
            username_avatar: AsyncLoad::new(|_| steam::name_avatar()),
            workshop_panel: WorkshopPanel::new(),
            download_panel: DownloadPanel::new(dest),
            whitelist_refresh: AsyncLoad::new(|_| whitelist::refresh_blocking()),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.show_loading_or_error(ui) {
            return;
        }
        steam::client().run_callbacks();
        let _ = self.whitelist_refresh.update(());
        steam::callbacks::flush_pending();
        self.show_nav_panel(ui);
        self.show_content_panel(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        settings::save_download_path(storage, self.download_panel.dest_path());
        storage.set_string(whitelist::STORAGE_KEY, whitelist::to_storage_string());
        storage.set_string(ignores::STORAGE_KEY, ignores::to_storage_string());
    }
}

impl App {
    fn show_loading_or_error(&mut self, ui: &mut egui::Ui) -> bool {
        if let Some(res) = self.loaded.update(()) {
            if let Err(e) = res.as_ref() {
                self.show_error_screen(ui, e);
                return false;
            }
            true
        } else {
            self.show_loading_screen(ui);
            false
        }
    }

    fn show_loading_screen(&self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.0 - 30.0);
                ui.spinner();
                ui.add_space(8.0);
                ui.label("Initializing Steam client...");
            });
        });
    }

    fn show_error_screen(&mut self, ui: &mut egui::Ui, error: &str) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.0 - 50.0);
                ui.label(
                    egui::RichText::new("Failed to initialize")
                        .size(24.0)
                        .color(egui::Color32::from_rgb(220, 80, 80)),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(error)
                        .size(14.0)
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(20.0);
                if ui.button("Retry").clicked() {
                    self.loaded.reset(());
                }
            });
        });
    }

    fn show_nav_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("nav_panel")
            .min_size(NAV_HEIGHT)
            .show_inside(ui, |ui| {
                ui.add_space(6.0);
                let bar = ui.max_rect();
                let row_h = NAV_HEIGHT - 12.0;

                let center_left = bar.center().x - TAB_BLOCK_W * 0.5;
                let center_right = bar.center().x + TAB_BLOCK_W * 0.5;

                let footer_right = bar.max.x - 12.0;
                let footer_left = (footer_right - FOOTER_W).max(center_right + 8.0);

                let left_rect = egui::Rect::from_min_max(
                    egui::pos2(bar.min.x + 6.0, bar.min.y),
                    egui::pos2(center_left - 8.0, bar.min.y + row_h),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(left_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| self.show_user_info(ui),
                );

                let center_rect = egui::Rect::from_center_size(
                    egui::pos2(bar.center().x, bar.min.y + row_h * 0.5),
                    egui::vec2(TAB_BLOCK_W, row_h),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(center_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| self.show_tab_buttons(ui),
                );

                let right_rect = egui::Rect::from_min_max(
                    egui::pos2(footer_left, bar.min.y),
                    egui::pos2(footer_right, bar.min.y + row_h),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(egui::Layout::top_down(egui::Align::RIGHT)),
                    |ui| self.show_footer(ui),
                );
            });
    }

    fn show_user_info(&mut self, ui: &mut egui::Ui) {
        let Some(avatar_res) = self.username_avatar.update(()) else {
            ui.add(Spinner::new().size(40.0));
            return;
        };
        ui.add(
            egui::Image::new(&avatar_res.avatar)
                .fit_to_exact_size(Vec2::new(44.0, 44.0))
                .corner_radius(44.0),
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&avatar_res.name).strong().size(18.0));
            ui.horizontal(|ui| {
                let status = steam::get_state();
                let status_color = status.color();
                let circle_radius = 5.0;
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(circle_radius * 2.0, circle_radius * 2.0),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .circle_filled(rect.center(), circle_radius, status_color);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(status.message().as_ref())
                            .size(12.0)
                            .color(status_color),
                    )
                    .truncate(),
                )
                .on_hover_text(status.message().as_ref());
            });
        });
    }

    fn show_tab_buttons(&mut self, ui: &mut egui::Ui) {
        if ui::tab_button(ui, "My Workshop", self.current_tab == Tab::MyWorkshop) {
            self.current_tab = Tab::MyWorkshop;
        }
        ui.add_space(8.0);
        if ui::tab_button(ui, "Download", self.current_tab == Tab::Download) {
            self.current_tab = Tab::Download;
        }
    }

    fn show_footer(&self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Made by Srlion")
                .color(BABY_BLUE)
                .size(12.0),
        );
        ui.add_space(2.0);

        let star = ui.add(
            egui::Button::new(
                egui::RichText::new("★ Star on GitHub")
                    .color(GOLD)
                    .size(13.0),
            )
            .corner_radius(6),
        );
        if star.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if star.clicked() {
            let _ = open::that("https://github.com/Srlion/gmod-wstool");
        }
        ui.add_space(2.0);

        ui.label(
            egui::RichText::new(env!("CARGO_PKG_VERSION"))
                .weak()
                .size(11.0),
        );
    }

    fn show_content_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| match self.current_tab {
            Tab::MyWorkshop => self.workshop_panel.show(ui),
            Tab::Download => self.download_panel.show(ui),
        });
    }
}
