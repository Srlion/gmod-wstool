use crate::async_load::AsyncLoad;
use crate::steam::download::{
    self, CollectionResolveJob, CopyUnpackJob, DownloadFinishJob, DownloadState, FinishResult,
    PollResult,
};
use crate::steam::items::fetch_title;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_virtual_list::VirtualList;
use std::path::{Path, PathBuf};

const MAX_ACTIVE: usize = 3;

struct Download {
    id: u64,
    state: DownloadState,
    title: AsyncLoad<Option<String>>,
    finish_job: Option<DownloadFinishJob>,
    copy_job: Option<CopyUnpackJob>,
    state_disc: std::mem::Discriminant<DownloadState>,
}

impl Download {
    fn new(id: u64, state: DownloadState) -> Self {
        let state_disc = std::mem::discriminant(&state);
        Self {
            id,
            state,
            title: AsyncLoad::new(move |_| fetch_title(id)),
            finish_job: None,
            copy_job: None,
            state_disc,
        }
    }
}

fn scan_items(dest_path: &Path) -> Vec<Download> {
    download::scan_existing(dest_path)
        .into_iter()
        .map(|id| Download::new(id, download::done_state(dest_path.join(id.to_string()))))
        .collect()
}

pub struct DownloadPanel {
    input: String,
    dest_input: String,
    dest_path: PathBuf,
    downloads: Vec<Download>,
    virtual_list: VirtualList,
    error: Option<String>,
    pending_collection: Option<CollectionResolveJob>,
}

impl DownloadPanel {
    pub fn new(dest_path: PathBuf) -> Self {
        let downloads = scan_items(&dest_path);
        Self {
            dest_input: dest_path.to_string_lossy().into_owned(),
            dest_path,
            input: String::new(),
            downloads,
            virtual_list: VirtualList::new(),
            error: None,
            pending_collection: None,
        }
    }

    pub fn dest_path(&self) -> &PathBuf {
        &self.dest_path
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(job) = self.pending_collection.as_mut() {
            let root_id = job.id();
            if let Some(res) = job.poll() {
                match res {
                    Ok(kids) if !kids.is_empty() => {
                        let kids = kids.clone();
                        self.pending_collection = None;
                        self.queue_one(root_id);
                        for kid in kids.into_iter().rev() {
                            self.queue_one(kid);
                        }
                    }
                    Ok(_) => {
                        self.pending_collection = None;
                        self.queue_one(root_id);
                    }
                    Err(e) => {
                        self.error = Some(e.clone());
                        self.pending_collection = None;
                    }
                }
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
        }

        let busy = self.pending_collection.is_some()
            || self.downloads.iter().any(|d| {
                matches!(
                    d.state,
                    DownloadState::Queued
                        | DownloadState::Pending
                        | DownloadState::Downloading { .. }
                )
            });
        if !busy && ui.input(|i| i.key_pressed(egui::Key::F5)) {
            self.downloads = scan_items(&self.dest_path);
            self.virtual_list.reset();
        }

        ui.add_space(4.0);

        let input_id = egui::Id::new("download_input");
        let input_focused = ui.ctx().memory(|m| m.has_focus(input_id));
        if !input_focused {
            let slash_pressed = ui.input_mut(|i| {
                let pressed = i.key_pressed(egui::Key::Slash);
                if pressed {
                    i.events
                        .retain(|e| !matches!(e, egui::Event::Text(t) if t == "/"));
                }
                pressed
            });
            if slash_pressed {
                let any_focused = ui.ctx().memory(|m| m.focused().is_some());
                if !any_focused {
                    ui.ctx().memory_mut(|m| m.request_focus(input_id));
                }
            }
        }

        let resolving = self.pending_collection.is_some();
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .id(input_id)
                    .desired_width(ui.available_width() - 170.0)
                    .hint_text("Workshop ID, URL, or collection... | Press / to focus"),
            );
            let submit = ui
                .add_enabled(
                    !resolving,
                    egui::Button::new("Download").min_size(Vec2::new(80.0, 26.0)),
                )
                .clicked()
                || (!resolving
                    && resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if submit {
                self.submit();
            }
            if ui
                .add_sized(Vec2::new(80.0, 26.0), egui::Button::new("Cancel All"))
                .clicked()
            {
                self.downloads = scan_items(&self.dest_path);
                self.virtual_list.reset();
            }
        });

        if resolving {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Resolving collection...").weak().size(12.0));
            });
        }

        if let Some(err) = &self.error {
            ui.add_space(6.0);
            ui.label(
                RichText::new(err)
                    .color(Color32::from_rgb(220, 80, 80))
                    .size(12.0),
            );
        }

        ui.add_space(12.0);

        let bottom_bar_height = 28.0;
        let available = ui.available_height() - bottom_bar_height - 8.0;

        let mut needs_repaint = false;
        let dest_path = self.dest_path.clone();

        // advance every in-flight job, visible or not
        for dl in self.downloads.iter_mut() {
            if matches!(
                dl.state,
                DownloadState::Done { .. } | DownloadState::Error(_)
            ) {
                continue;
            }
            if let Some(job) = dl.copy_job.as_mut() {
                if let Some(state) = job.poll() {
                    dl.state = state.clone();
                    dl.copy_job = None;
                }
                needs_repaint = true;
            } else if let Some(job) = dl.finish_job.as_mut() {
                match job.poll() {
                    Some(FinishResult::Installed(src)) => {
                        let src = src.clone();
                        dl.finish_job = None;
                        dl.copy_job = Some(CopyUnpackJob::start(src, dl.id, dest_path.clone()));
                        needs_repaint = true;
                    }
                    Some(FinishResult::Failed(e)) => {
                        dl.state = DownloadState::Error(e.clone());
                        dl.finish_job = None;
                        needs_repaint = true;
                    }
                    None => {
                        if let PollResult::Downloading(s) = download::poll_progress(dl.id) {
                            dl.state = s;
                        }
                        needs_repaint = true;
                    }
                }
            }
        }
        self.pump_queue();

        let mut layout_changed = false;
        for dl in self.downloads.iter_mut() {
            let disc = std::mem::discriminant(&dl.state);
            if disc != dl.state_disc {
                dl.state_disc = disc;
                layout_changed = true;
            }
        }
        if layout_changed {
            self.virtual_list.reset();
        }

        let downloads = &mut self.downloads;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(available)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.virtual_list
                    .ui_custom_layout(ui, downloads.len(), |ui, start_index| {
                        let dl = &mut downloads[start_index];
                        let title = dl.title.update(());
                        if title.is_none() {
                            needs_repaint = true;
                        }
                        let t = title.as_deref().and_then(|x| x.as_deref());
                        show_item(ui, dl, t);
                        ui.add_space(4.0);
                        1
                    });
            });

        if needs_repaint {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Save to:");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.dest_input)
                            .desired_width(ui.available_width()),
                    )
                    .changed()
                {
                    self.dest_path = PathBuf::from(&self.dest_input);
                }
            });
        });
    }

    fn submit(&mut self) {
        self.error = None;
        let Some(id) = download::parse_id(&self.input) else {
            self.error = Some("Couldn't find a valid ID in that input.".into());
            return;
        };
        self.pending_collection = Some(CollectionResolveJob::start(id));
        self.input.clear();
    }

    fn queue_one(&mut self, id: u64) {
        if self.downloads.iter().any(|d| d.id == id) {
            return;
        }
        self.downloads
            .insert(0, Download::new(id, DownloadState::Queued));
        self.virtual_list.reset();
    }

    fn pump_queue(&mut self) {
        let active = self
            .downloads
            .iter()
            .filter(|d| {
                matches!(
                    d.state,
                    DownloadState::Pending | DownloadState::Downloading { .. }
                )
            })
            .count();
        let mut free = MAX_ACTIVE.saturating_sub(active);
        for dl in self.downloads.iter_mut() {
            if free == 0 {
                break;
            }
            if matches!(dl.state, DownloadState::Queued) {
                match download::start_download(dl.id) {
                    Ok(job) => {
                        dl.finish_job = Some(job);
                        dl.state = DownloadState::Pending;
                        free -= 1;
                    }
                    Err(e) => dl.state = DownloadState::Error(e),
                }
            }
        }
    }
}

fn show_item(ui: &mut egui::Ui, dl: &Download, title: Option<&str>) {
    let error = matches!(dl.state, DownloadState::Error(_));
    let dot_color = if error {
        Color32::from_rgb(220, 80, 80)
    } else if matches!(dl.state, DownloadState::Done { .. }) {
        Color32::from_rgb(80, 200, 120)
    } else if matches!(dl.state, DownloadState::Queued) {
        Color32::from_gray(110)
    } else {
        Color32::YELLOW
    };

    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_height(26.0);
                let dot_r = 5.0;
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(dot_r * 2.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), dot_r, dot_color);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let url = format!(
                        "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                        dl.id
                    );
                    if ui
                        .add_sized(Vec2::new(80.0, 26.0), egui::Button::new("Workshop"))
                        .clicked()
                    {
                        let _ = open::that(&url);
                    }
                    if let DownloadState::Done { unpacked, .. } = &dl.state
                        && let Some(dir) = unpacked
                        && ui
                            .add_sized(Vec2::new(95.0, 26.0), egui::Button::new("Open Folder"))
                            .clicked()
                    {
                        let target = dir.parent().unwrap_or(dir);
                        let _ = open::that(target);
                    }
                    ui.vertical(|ui| match title {
                        Some(t) => {
                            let mut job = egui::text::LayoutJob::default();
                            let c = ui.visuals().strong_text_color();
                            let w = ui.visuals().weak_text_color();
                            job.append(
                                t,
                                0.0,
                                egui::TextFormat {
                                    color: c,
                                    ..Default::default()
                                },
                            );
                            job.append(
                                &format!("  #{}", dl.id),
                                0.0,
                                egui::TextFormat {
                                    color: w,
                                    ..Default::default()
                                },
                            );
                            job.wrap.max_width = ui.available_width();
                            ui.label(job);
                        }
                        None => {
                            ui.label(RichText::new(format!("#{}", dl.id)).strong());
                        }
                    });
                });
            });

            match &dl.state {
                DownloadState::Queued => {
                    ui.add_space(2.0);
                    ui.label(RichText::new("Waiting in queue...").weak().size(11.0));
                }
                DownloadState::Downloading { .. } => {
                    ui.add_space(6.0);
                    ui.add(
                        egui::ProgressBar::new(dl.state.fraction())
                            .show_percentage()
                            .desired_width(ui.available_width()),
                    );
                }
                DownloadState::Pending => {
                    ui.add_space(2.0);
                    ui.label(RichText::new("Queued...").weak().size(11.0));
                }
                DownloadState::Done { gma, unpacked } => {
                    ui.add_space(2.0);
                    if let Some(dir) = unpacked {
                        ui.label(RichText::new(dir.to_string_lossy()).weak().size(11.0));
                    } else {
                        ui.label(RichText::new(gma.to_string_lossy()).weak().size(11.0));
                    }
                }
                DownloadState::Error(e) => {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(e)
                            .color(Color32::from_rgb(220, 80, 80))
                            .size(11.0),
                    );
                }
            }
        });
}
