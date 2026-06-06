use crate::{
    async_load::AsyncLoad,
    steam::{
        items::{WorkshopItem, WorkshopQueryResult, query_items},
        update::{self, CreateJob, CreateState, UpdateJob, UpdateRequest, UpdateState},
    },
};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use egui::scroll_area::ScrollSource;
use egui_notify::{Anchor, Toasts};
use std::path::PathBuf;
use std::time::Duration;

const THUMB: f32 = 96.0;
const ROW_PAD: f32 = 12.0;
const ROW_GAP: f32 = 8.0;
const MAX_ROW_W: f32 = 720.0;
const ACCENT: Color32 = Color32::from_rgb(100, 149, 237);
const STAR: Color32 = Color32::from_rgb(255, 200, 60);

const ADDON_TYPES: &[&str] = &[
    "gamemode",
    "map",
    "tool",
    "weapon",
    "effects",
    "vehicle",
    "npc",
    "entity",
    "model",
    "servercontent",
];
const ADDON_TAGS: &[&str] = &[
    "movie", "build", "water", "fun", "roleplay", "scenic", "realism", "cartoon", "comic",
];

#[derive(Clone, PartialEq)]
struct Fields {
    title: String,
    description: String,
    visibility: u8,
    addon_type: String,
    selected_tags: std::collections::BTreeSet<String>,
    content_path: String,
    preview_path: String,
    change_note: String,
    fix_size: bool,
    extra_ignores: String,
}

struct Editor {
    id: u64,
    cur: Fields,
    original: Fields,
    job: Option<UpdateJob>,
    error: Option<String>,
    confirm_submit: bool,
    show_files_popup: bool,
    show_preview_popup: bool,
    preview_tex: Option<egui::TextureHandle>,
    preview_err: Option<String>,
    preview_built_from: Option<(String, bool)>,
}

impl Editor {
    fn is_dirty(&self) -> bool {
        self.cur != self.original
    }

    fn from_item(item: &WorkshopItem, content_guess: Option<PathBuf>) -> Self {
        let cur = Fields {
            title: item.title.clone(),
            description: item.description.clone(),
            visibility: item.visibility,
            addon_type: item
                .tags
                .iter()
                .map(|t| t.to_lowercase())
                .find(|t| ADDON_TYPES.contains(&t.as_str()))
                .unwrap_or_default(),
            selected_tags: item
                .tags
                .iter()
                .map(|t| t.to_lowercase())
                .filter(|t| ADDON_TAGS.contains(&t.as_str()))
                .collect(),
            content_path: content_guess
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            preview_path: String::new(),
            change_note: String::new(),
            fix_size: false,
            extra_ignores: String::new(),
        };
        Self {
            id: item.id,
            original: cur.clone(),
            cur,
            job: None,
            error: None,
            confirm_submit: false,
            show_files_popup: false,
            show_preview_popup: false,
            preview_tex: None,
            preview_err: None,
            preview_built_from: None,
        }
    }
}

pub struct WorkshopPanel {
    loader: AsyncLoad<Result<WorkshopQueryResult, String>, u32>,
    toasts: Toasts,
    page: u32,
    editor: Option<Editor>,
    confirm_leave: bool,
    create_job: Option<CreateJob>,
    show_ignores: bool,
    ignores_buf: String,
}

impl WorkshopPanel {
    pub fn new() -> Self {
        Self {
            loader: AsyncLoad::new(query_items),
            toasts: Toasts::default()
                .with_anchor(Anchor::BottomRight)
                .with_margin(egui::vec2(50.0, 50.0))
                .with_shadow(egui::epaint::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                }),
            page: 1,
            editor: None,
            confirm_leave: false,
            create_job: None,
            show_ignores: false,
            ignores_buf: String::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(job) = self.create_job.as_mut() {
            match job.poll() {
                CreateState::Creating => {
                    ui.ctx().request_repaint();
                }
                CreateState::Done {
                    needs_legal_agreement,
                } => {
                    let needs = *needs_legal_agreement;
                    self.create_job = None;
                    self.loader.reset(self.page);
                    self.toasts
                        .success("Addon created!")
                        .duration(Some(Duration::from_secs(3)));
                    if needs {
                        self.toasts
                            .warning(
                                "Accept the Workshop legal agreement on Steam for it to go live.",
                            )
                            .duration(Some(Duration::from_secs(4)));
                    }
                }
                CreateState::Error(e) => {
                    self.toasts
                        .error(format!("Failed to create item: {e}"))
                        .duration(Some(Duration::from_secs(4)));
                    self.create_job = None;
                }
            }
        }

        if self.editor.is_some() {
            self.show_editor(ui);
            self.toasts.show(ui.ctx());
            return;
        }
        self.show_list(ui);
        self.toasts.show(ui.ctx());
    }

    fn show_list(&mut self, ui: &mut egui::Ui) {
        let data = self.loader.update(self.page);
        let loading = data.is_none();
        ui.horizontal(|ui| {
            let f5 = ui.input(|i| i.key_pressed(egui::Key::F5));
            if f5 && !loading {
                self.loader.reset(self.page);
            }
            if let Some(Ok(r)) = data.as_deref() {
                ui.label(
                    RichText::new(format!(
                        "{} item{}",
                        r.total_results,
                        if r.total_results == 1 { "" } else { "s" }
                    ))
                    .weak(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.create_job.is_some();
                if ui
                    .add_enabled(!busy, egui::Button::new("+ New Addon"))
                    .clicked()
                {
                    self.create_job = Some(CreateJob::start());
                }
                let btn = egui::Button::new(RichText::new("⛔ Ignore list").color(Color32::WHITE))
                    .fill(Color32::from_rgb(180, 60, 60));
                if ui.add(btn).clicked() {
                    self.ignores_buf = crate::ignores::get().join("\n");
                    self.show_ignores = true;
                }
            });
        });
        ui.add_space(8.0);
        let pagination_height = if data.as_deref().is_some_and(|r| r.is_ok()) {
            44.0
        } else {
            0.0
        };
        let available = ui.available_height() - pagination_height;

        let mut clicked: Option<usize> = None;

        egui::ScrollArea::vertical()
            .id_salt("list_scroll")
            .auto_shrink([false, false])
            .max_height(available)
            .scroll_source(ScrollSource::SCROLL_BAR | ScrollSource::MOUSE_WHEEL)
            .show(ui, |ui| {
                let w = ui.available_width().min(MAX_ROW_W);
                let x_off = ((ui.available_width() - w) * 0.5).max(0.0);
                ui.add_space(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(x_off);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        match data.as_deref() {
                            None => {
                                for _ in 0..6 {
                                    skeleton_row(ui);
                                    ui.add_space(ROW_GAP);
                                }
                            }
                            Some(Err(e)) => message(
                                ui,
                                Color32::from_rgb(60, 30, 30),
                                &format!("Failed to load: {e}"),
                            ),
                            Some(Ok(r)) if r.items.is_empty() && r.page == 1 => {
                                message(ui, ui.visuals().faint_bg_color, "No workshop items found.")
                            }
                            Some(Ok(r)) => {
                                for (idx, item) in r.items.iter().enumerate() {
                                    if item_row(ui, item, &mut self.toasts) {
                                        clicked = Some(idx);
                                    }
                                    ui.add_space(ROW_GAP);
                                }
                            }
                        }
                    });
                });
            });

        if let (Some(idx), Some(Ok(r))) = (clicked, data.as_deref()) {
            if let Some(item) = r.items.get(idx) {
                self.editor = Some(Editor::from_item(item, None));
            }
        }

        if let Some(Ok(r)) = data.as_deref() {
            ui.add_space(8.0);
            let prev_w = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(egui::Id::new("pager_w")).unwrap_or(0.0));
            let x_off = ((ui.available_width() - prev_w) * 0.5).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(x_off);
                let start = ui.cursor().min.x;
                if ui
                    .add_enabled(self.page > 1 && !loading, egui::Button::new("<  Prev"))
                    .clicked()
                {
                    self.page -= 1;
                    self.loader.reset(self.page);
                }
                ui.label(RichText::new(format!("Page {} of {}", self.page, r.total_pages)).weak());
                if ui
                    .add_enabled(
                        self.page < r.total_pages && !loading,
                        egui::Button::new("Next  >"),
                    )
                    .clicked()
                {
                    self.page += 1;
                    self.loader.reset(self.page);
                }
                let measured = ui.cursor().min.x - start;
                if (measured - prev_w).abs() > 0.5 {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(egui::Id::new("pager_w"), measured);
                    });
                    ui.ctx().request_repaint();
                }
            });
        }
        self.show_ignores_modal(ui);
        if loading {
            ui.ctx().request_repaint();
        }
    }

    fn show_editor(&mut self, ui: &mut egui::Ui) {
        let mut job_state: Option<UpdateState> = None;
        if let Some(e) = self.editor.as_mut() {
            if let Some(job) = e.job.as_mut() {
                job_state = Some(job.poll());
                ui.ctx().request_repaint();
            }
        }
        if let Some(UpdateState::Done {
            needs_legal_agreement,
        }) = job_state
        {
            if let Some(e) = self.editor.as_mut() {
                e.cur.content_path.clear();
                e.cur.preview_path.clear();
                e.cur.change_note.clear();
                e.original = e.cur.clone();
                e.job = None;
            }
            self.loader.reset(self.page);
            self.toasts
                .success("Update submitted!")
                .duration(Some(Duration::from_secs(3)));
            if needs_legal_agreement {
                self.toasts
                    .warning("Accept the Workshop legal agreement on Steam for it to go live.")
                    .duration(Some(Duration::from_secs(3)));
            }
        }
        let uploading = matches!(job_state, Some(UpdateState::Uploading));

        let mut go_back = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!uploading, egui::Button::new("<  Back"))
                .clicked()
            {
                if self.editor.as_ref().is_some_and(|e| e.is_dirty()) {
                    self.confirm_leave = true;
                } else {
                    go_back = true;
                }
            }
            if let Some(e) = &self.editor {
                ui.label(RichText::new(format!("Editing #{}", e.id)).weak());
                if e.is_dirty() {
                    ui.label(RichText::new("• unsaved changes").color(STAR).size(12.0));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let url = format!(
                        "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                        e.id
                    );
                    let link = ui.add(
                        egui::Label::new(
                            RichText::new("open in browser")
                                .size(14.0)
                                .color(ACCENT)
                                .underline(),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if link.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if link.clicked() {
                        let _ = open::that(&url);
                    }
                });
            }
        });
        if self.confirm_leave {
            ui.add_space(4.0);
            egui::Frame::NONE
                .fill(Color32::from_gray(40))
                .corner_radius(6)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("You have unsaved changes.").color(STAR));
                        if ui.button("Discard").clicked() {
                            go_back = true;
                            self.confirm_leave = false;
                        }
                        if ui.button("Keep editing").clicked() {
                            self.confirm_leave = false;
                        }
                    });
                });
        }
        if go_back {
            self.editor = None;
            self.confirm_leave = false;
            return;
        }

        let editor = self.editor.as_mut().unwrap();

        ui.add_space(8.0);
        let w = ui.available_width().min(MAX_ROW_W);
        let x_off = ((ui.available_width() - w) * 0.5).max(0.0);

        egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(x_off);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        show_editor_fields(ui, editor, uploading);
                        show_editor_status(ui, &job_state, &editor.error);
                        show_submit_button(ui, editor, uploading);
                        show_confirm_submit_modal(ui, editor);
                        show_files_modal(ui, editor);
                        show_preview_modal(ui, editor);
                    });
                });
            });
    }

    fn show_ignores_modal(&mut self, ui: &mut egui::Ui) {
        if !self.show_ignores {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("ignore_list")).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Global ignore list").strong().size(15.0));
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "One glob per line, skipped when packing any addon. Case-insensitive.",
                )
                .weak()
                .size(12.0),
            );
            ui.add_space(12.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.ignores_buf)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .hint_text("*.psd\nsrc/*\nnotes.txt"),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Always ignored:\n{}",
                    crate::steam::pack::DEFAULT_IGNORES.join("\n")
                ))
                .weak()
                .size(11.0),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    crate::ignores::set(self.ignores_buf.lines().map(|l| l.to_string()).collect());
                    self.toasts
                        .success("Ignore list saved")
                        .duration(Some(Duration::from_secs(2)));
                    self.show_ignores = false;
                }
                if ui.button("Cancel").clicked() {
                    self.show_ignores = false;
                }
            });
        });
        if modal.should_close() {
            self.show_ignores = false;
        }
    }
}

fn show_editor_fields(ui: &mut egui::Ui, editor: &mut Editor, uploading: bool) {
    ui.add_enabled_ui(!uploading, |ui| {
        field_label(ui, "Title", editor.cur.title != editor.original.title);
        dirty_frame(ui, editor.cur.title != editor.original.title, |ui| {
            ui.add(egui::TextEdit::singleline(&mut editor.cur.title).desired_width(f32::INFINITY));
        });
        ui.add_space(8.0);

        field_label(
            ui,
            "Description",
            editor.cur.description != editor.original.description,
        );
        dirty_frame(
            ui,
            editor.cur.description != editor.original.description,
            |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut editor.cur.description)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
            },
        );
        ui.add_space(8.0);

        field_label(
            ui,
            "Visibility",
            editor.cur.visibility != editor.original.visibility,
        );
        dirty_frame(
            ui,
            editor.cur.visibility != editor.original.visibility,
            |ui| {
                ui.horizontal(|ui| {
                    for (val, label) in [
                        (update::VIS_PUBLIC, "Public"),
                        (update::VIS_FRIENDS, "Friends"),
                        (update::VIS_PRIVATE, "Private"),
                        (update::VIS_UNLISTED, "Unlisted"),
                    ] {
                        ui.selectable_value(&mut editor.cur.visibility, val, label);
                    }
                });
            },
        );
        ui.add_space(8.0);

        field_label(
            ui,
            "Type",
            editor.cur.addon_type != editor.original.addon_type,
        );
        dirty_frame(
            ui,
            editor.cur.addon_type != editor.original.addon_type,
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    for &t in ADDON_TYPES {
                        ui.selectable_value(&mut editor.cur.addon_type, t.to_string(), t);
                    }
                });
            },
        );
        ui.add_space(8.0);

        field_label(
            ui,
            "Tags",
            editor.cur.selected_tags != editor.original.selected_tags,
        );
        dirty_frame(
            ui,
            editor.cur.selected_tags != editor.original.selected_tags,
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    for &t in ADDON_TAGS {
                        let mut on = editor.cur.selected_tags.contains(t);
                        if ui.checkbox(&mut on, t).changed() {
                            if on {
                                editor.cur.selected_tags.insert(t.to_string());
                            } else {
                                editor.cur.selected_tags.remove(t);
                            }
                        }
                    }
                });
            },
        );
        ui.add_space(8.0);

        let content_changed = editor.cur.content_path != editor.original.content_path;
        let preview_changed = editor.cur.preview_path != editor.original.preview_path;

        field_label(ui, "Files", content_changed);
        ui.horizontal(|ui| {
            if ui.button("Update files...").clicked() {
                editor.show_files_popup = true;
            }
            if content_changed {
                ui.label(
                    RichText::new("folder staged for upload")
                        .color(STAR)
                        .size(12.0),
                );
            } else {
                ui.label(RichText::new("no file changes").weak().size(12.0));
            }
        });
        ui.add_space(8.0);

        field_label(ui, "Preview image (512x512)", preview_changed);
        ui.horizontal(|ui| {
            if ui.button("Update preview...").clicked() {
                editor.show_preview_popup = true;
            }
            if preview_changed {
                ui.label(
                    RichText::new("image staged for upload")
                        .color(STAR)
                        .size(12.0),
                );
            } else {
                ui.label(RichText::new("no preview change").weak().size(12.0));
            }
        });
    });
}

fn show_editor_status(ui: &mut egui::Ui, job_state: &Option<UpdateState>, error: &Option<String>) {
    ui.add_space(12.0);

    match job_state {
        Some(UpdateState::Uploading) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Uploading to Steam...");
            });
        }
        Some(UpdateState::Done {
            needs_legal_agreement,
        }) => {
            ui.label(RichText::new("Update submitted.").color(Color32::from_rgb(80, 200, 120)));
            if *needs_legal_agreement {
                ui.label(
                    RichText::new(
                        "You must accept the Workshop legal agreement \
                                    on the item's Steam page for it to go live.",
                    )
                    .color(STAR)
                    .size(12.0),
                );
            }
        }
        Some(UpdateState::Error(e)) => {
            ui.label(
                RichText::new(format!("Update failed: {e}")).color(Color32::from_rgb(220, 80, 80)),
            );
        }
        None => {}
    }

    if let Some(err) = error {
        ui.label(
            RichText::new(err)
                .color(Color32::from_rgb(220, 80, 80))
                .size(12.0),
        );
    }
}

fn show_submit_button(ui: &mut egui::Ui, editor: &mut Editor, uploading: bool) {
    ui.add_space(6.0);
    let dirty = editor.is_dirty();
    if !dirty {
        editor.confirm_submit = false;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(16.0, 8.0);
        if ui
            .add_enabled(!uploading && dirty, egui::Button::new("Submit Update"))
            .clicked()
        {
            editor.confirm_submit = true;
        }
    });
}

fn show_confirm_submit_modal(ui: &mut egui::Ui, editor: &mut Editor) {
    if !editor.confirm_submit {
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("confirm_submit")).show(ui.ctx(), |ui| {
        ui.set_width(280.0);
        ui.label(
            RichText::new("Submit this update to Steam?")
                .strong()
                .size(15.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("This pushes your changes live.")
                .weak()
                .size(12.0),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Confirm").clicked() {
                editor.confirm_submit = false;
                editor.error = None;
                match build_request(editor) {
                    Ok(req) => editor.job = Some(UpdateJob::start(req)),
                    Err(e) => editor.error = Some(e),
                }
            }
            if ui.button("Cancel").clicked() {
                editor.confirm_submit = false;
            }
        });
    });
    if modal.should_close() {
        editor.confirm_submit = false;
    }
}

fn show_files_modal(ui: &mut egui::Ui, editor: &mut Editor) {
    if !editor.show_files_popup {
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("update_files")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new("Update files").strong().size(15.0));
        ui.add_space(2.0);
        ui.label(
            RichText::new("Leave empty to keep the current version on Steam.")
                .weak()
                .size(12.0),
        );
        ui.add_space(12.0);

        field_label(
            ui,
            "Content folder (the unpacked gma)",
            editor.cur.content_path != editor.original.content_path,
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut editor.cur.content_path)
                    .desired_width(ui.available_width() - 90.0),
            );
            if ui.button("Browse").clicked() {
                let mut dialog = rfd::FileDialog::new();
                let cur = editor.cur.content_path.trim();
                if !cur.is_empty() {
                    let p = PathBuf::from(cur);
                    if p.is_dir() {
                        dialog = dialog.set_directory(&p);
                    }
                }
                if let Some(dir) = dialog.pick_folder() {
                    editor.cur.content_path = dir.to_string_lossy().into_owned();
                }
            }
        });
        ui.label(
            RichText::new("We pack this folder into a .gma and upload that.")
                .weak()
                .size(11.0),
        );
        ui.add_space(10.0);

        ui.label(RichText::new("Change note").strong());
        ui.add(
            egui::TextEdit::multiline(&mut editor.cur.change_note)
                .desired_width(f32::INFINITY)
                .desired_rows(2)
                .hint_text("What changed in this update?"),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            if ui.button("Done").clicked() {
                editor.show_files_popup = false;
            }
            if ui.button("Clear folder").clicked() {
                editor.cur.content_path = editor.original.content_path.clone();
            }
        });
    });
    if modal.should_close() {
        editor.show_files_popup = false;
    }
}

fn show_preview_modal(ui: &mut egui::Ui, editor: &mut Editor) {
    if !editor.show_preview_popup {
        return;
    }
    let modal = egui::Modal::new(egui::Id::new("update_preview")).show(
        ui.ctx(),
        |ui| {
            ui.set_width(420.0);
            ui.label(
                RichText::new("Update preview image").strong().size(15.0),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Leave empty to keep the current image on Steam.",
                )
                .weak()
                .size(12.0),
            );
            ui.add_space(12.0);

            field_label(
                ui,
                "Preview image",
                editor.cur.preview_path != editor.original.preview_path,
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(
                        &mut editor.cur.preview_path,
                    )
                    .desired_width(ui.available_width() - 90.0),
                );
                if ui.button("Browse").clicked() {
                    let mut dialog = rfd::FileDialog::new().add_filter(
                        "image",
                        &["jpg", "jpeg", "png", "gif"],
                    );
                    let cur = editor.cur.preview_path.trim();
                    if !cur.is_empty() {
                        let p = PathBuf::from(cur);
                        if let Some(parent) = p.parent() {
                            if parent.is_dir() {
                                dialog = dialog.set_directory(parent);
                            }
                        }
                        if let Some(name) = p.file_name() {
                            dialog = dialog
                                .set_file_name(name.to_string_lossy());
                        }
                    }
                    if let Some(file) = dialog.pick_file() {
                        editor.cur.preview_path = file.to_string_lossy().into_owned();
                        editor.preview_tex = None;
                        editor.preview_err = None;
                    }
                }
            });
            ui.add_space(10.0);
            ui.checkbox(&mut editor.cur.fix_size, "Resize image to fit 512x512")
                .on_hover_text("Scales the image down to fit inside 512x512 and pads the rest with transparency.");
            let path = editor.cur.preview_path.trim().to_string();
            let want = if !path.is_empty() {
                Some((path.clone(),editor.cur.fix_size))
            } else {
                None
            };
            if want != editor.preview_built_from {
                editor.preview_tex = None;
                editor.preview_err = None;
                editor.preview_built_from = want.clone();
                if want.is_some() {
                    let res = if editor.cur.fix_size {
                        make_512(std::path::Path::new(&path))
                    } else {
                        load_rgba(std::path::Path::new(&path))
                    };
                    match res {
                        Ok(img) => {
                            let size = [img.width() as usize, img.height() as usize];
                            if !editor.cur.fix_size && size != [512, 512]{
                                editor.preview_err = Some(format!("This image is {}x{}, not 512x512. Tick the box above to resize it.",size[0],size[1]));
                            }
                            let color = egui::ColorImage::from_rgba_unmultiplied(size, &img.into_raw());
                            editor.preview_tex = Some(ui.ctx().load_texture("preview512", color, egui::TextureOptions::LINEAR));
                        }
                        Err(e) => editor.preview_err = Some(e),
                    }
                }
            }
            if let Some(e) = &editor.preview_err {
                ui.add_space(4.0);
                ui.label(RichText::new(e).color(Color32::from_rgb(220, 80, 80)).size(11.0));
            }
            if let Some(tex) = &editor.preview_tex {
                ui.add_space(8.0);
                let caption = if editor.cur.fix_size { "Result (512x512):" } else { "Original (uploaded as-is):" };
                ui.label(RichText::new(caption).weak().size(11.0));
                ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(160.0)).corner_radius(6));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Done").clicked() {
                    editor.show_preview_popup = false;
                }
                if ui.button("Clear image").clicked() {
                    editor.cur.preview_path = editor.original.preview_path.clone();
                    editor.preview_tex = None;
                    editor.preview_err = None;
                }
            });
        },
    );
    if modal.should_close() {
        editor.show_preview_popup = false;
    }
}

fn make_512(src: &std::path::Path) -> Result<image::RgbaImage, String> {
    let img = image::open(src).map_err(|e| format!("Open image failed: {e}"))?;
    const N: u32 = 512;
    let scaled = img.resize(N, N, image::imageops::FilterType::Lanczos3);
    let mut canvas = image::RgbaImage::new(N, N); // new() is fully transparent
    let (w, h) = (scaled.width(), scaled.height());
    let (ox, oy) = ((N - w) / 2, (N - h) / 2);
    image::imageops::overlay(&mut canvas, &scaled.to_rgba8(), ox as i64, oy as i64);
    Ok(canvas)
}

fn load_rgba(src: &std::path::Path) -> Result<image::RgbaImage, String> {
    let img = image::open(src).map_err(|e| format!("Open image failed: {e}"))?;
    Ok(img.to_rgba8())
}

fn build_request(e: &Editor) -> Result<UpdateRequest, String> {
    let mut tags: Vec<String> = Vec::new();
    if !e.cur.addon_type.is_empty() {
        tags.push(e.cur.addon_type.clone());
    }
    tags.extend(e.cur.selected_tags.iter().cloned());

    let content_path = if e.cur.content_path.trim().is_empty() {
        None
    } else {
        let src = PathBuf::from(e.cur.content_path.trim());
        if !src.is_dir() {
            return Err("Content path must be an existing folder.".into());
        }
        // pack the folder into a gma ourselves, hand Steam the temp dir
        Some(crate::steam::pack::pack_to_temp_dir(
            e.cur.title.clone(),
            &src,
        )?)
    };

    let preview_path = if e.cur.preview_path.trim().is_empty() {
        None
    } else {
        let p = PathBuf::from(e.cur.preview_path.trim());
        if !p.is_file() {
            return Err("Preview path must be an existing file.".into());
        }
        if e.cur.fix_size {
            let img = make_512(&p)?;
            let dir =
                std::env::temp_dir().join(format!("gmod-wstool-preview-{}", std::process::id()));
            std::fs::create_dir_all(&dir)
                .map_err(|err| format!("Create temp dir failed: {err}"))?;
            let out = dir.join("preview.png");
            img.save(&out)
                .map_err(|err| format!("Save preview failed: {err}"))?;
            Some(out)
        } else {
            let (w, h) =
                image::image_dimensions(&p).map_err(|err| format!("Open image failed: {err}"))?;
            if (w, h) != (512, 512) {
                return Err(format!(
                    "Preview must be 512x512 (this is {w}x{h}). Tick \"Resize image to fit 512x512\" to fix it automatically."
                ));
            }
            Some(p)
        }
    };

    Ok(UpdateRequest {
        id: e.id,
        title: Some(e.cur.title.clone()),
        description: Some(e.cur.description.clone()),
        visibility: e.cur.visibility,
        tags: Some(tags),
        content_path,
        preview_path,
        change_note: e.cur.change_note.clone(),
    })
}

fn message(ui: &mut egui::Ui, color: Color32, msg: &str) {
    egui::Frame::NONE
        .fill(color)
        .corner_radius(6)
        .inner_margin(12.0)
        .show(ui, |ui| ui.label(msg));
}

fn row_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(Color32::from_gray(28))
        .corner_radius(8)
        .stroke(Stroke::new(1.0, Color32::from_gray(55)))
        .inner_margin(ROW_PAD)
}

fn skeleton_row(ui: &mut egui::Ui) {
    let ph = Color32::from_gray(45);
    row_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(THUMB), egui::Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(6), ph);
            ui.add_space(ROW_PAD);
            ui.vertical(|ui| {
                for (frac, h) in [(0.6, 14.0), (0.35, 11.0), (0.5, 11.0)] {
                    let (r, _) = ui.allocate_exact_size(
                        Vec2::new((ui.available_width()) * frac, h),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(r, CornerRadius::same(3), ph);
                    ui.add_space(8.0);
                }
            });
        });
    });
}

fn item_row(ui: &mut egui::Ui, item: &WorkshopItem, toasts: &mut Toasts) -> bool {
    let mut row_clicked = false;
    row_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (thumb_rect, _) = ui.allocate_exact_size(Vec2::splat(THUMB), egui::Sense::hover());
            match &item.preview_url {
                Some(url) => {
                    ui.put(
                        thumb_rect,
                        egui::Image::new(url)
                            .fit_to_exact_size(thumb_rect.size())
                            .corner_radius(CornerRadius::same(6))
                            .texture_options(egui::TextureOptions::LINEAR),
                    );
                }
                None => {
                    ui.painter().rect_filled(
                        thumb_rect,
                        CornerRadius::same(6),
                        Color32::from_gray(40),
                    );
                    ui.painter().text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "No\nPreview",
                        egui::FontId::proportional(11.0),
                        Color32::from_gray(95),
                    );
                }
            }
            ui.add_space(ROW_PAD);
            ui.vertical(|ui| {
                ui.set_min_height(THUMB);
                ui.add(egui::Label::new(RichText::new(&item.title).strong().size(15.0)).truncate());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let total = item.num_upvotes + item.num_downvotes;
                    if total >= 10 {
                        let stars = item.num_upvotes as f32 / total as f32 * 5.0;
                        ui.label(
                            RichText::new(format!("{stars:.1} *"))
                                .size(12.0)
                                .color(STAR),
                        );
                        ui.label(RichText::new(format!("({total})")).size(12.0).weak());
                        ui.label(RichText::new("|").size(12.0).weak());
                    }
                    ui.label(RichText::new(fmt_size(item.file_size)).size(12.0).weak());
                    if let Some(subs) = item.subscriptions {
                        ui.label(RichText::new("|").size(12.0).weak());
                        ui.label(
                            RichText::new(format!("{} subs", fmt_count(subs)))
                                .size(12.0)
                                .weak(),
                        );
                    }
                });
                if !item.tags.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
                        for tag in item.tags.iter().take(6) {
                            egui::Frame::NONE
                                .fill(Color32::from_gray(42))
                                .corner_radius(CornerRadius::same(10))
                                .inner_margin(egui::Margin::symmetric(7, 3))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(tag)
                                            .size(11.0)
                                            .color(Color32::from_gray(195)),
                                    );
                                });
                        }
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let id_resp = ui.add(
                        egui::Label::new(
                            RichText::new(format!("#{}", item.id))
                                .size(12.0)
                                .color(ACCENT)
                                .underline(),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if id_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if id_resp.on_hover_text("Click to copy ID").clicked() {
                        ui.ctx().copy_text(item.id.to_string());
                        toasts
                            .success("Copied ID!")
                            .duration(Some(Duration::from_secs(2)));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let url = format!(
                            "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                            item.id
                        );
                        if ui
                            .add_sized(Vec2::new(80.0, 26.0), egui::Button::new("Workshop"))
                            .clicked()
                        {
                            let _ = open::that(&url);
                        }
                        if ui
                            .add_sized(Vec2::new(70.0, 26.0), egui::Button::new("Edit"))
                            .clicked()
                        {
                            row_clicked = true;
                        }
                    });
                });
            });
        });
    });
    row_clicked
}

fn fmt_size(bytes: u32) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    let i = ((bytes as f64).log2() / 10.0) as usize;
    format!(
        "{:.1} {}",
        bytes as f64 / (1u64 << (i * 10)) as f64,
        ["B", "KB", "MB", "GB"][i.min(3)]
    )
}

fn fmt_count(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{:.1}K", n as f64 / 1_000.0),
        _ => n.to_string(),
    }
}

fn field_label(ui: &mut egui::Ui, text: &str, changed: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).strong());
        if changed {
            ui.label(RichText::new("•").color(STAR));
        }
    });
}

fn dirty_frame<R>(
    ui: &mut egui::Ui,
    changed: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let color = if changed { STAR } else { Color32::TRANSPARENT };
    egui::Frame::NONE
        .inner_margin(2.0)
        .stroke(Stroke::new(1.0, color))
        .corner_radius(4)
        .show(ui, add_contents)
        .inner
}
