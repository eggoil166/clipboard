use eframe::egui;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::models::ClipSummary;
use crate::storage::Database;
use crate::cloudstorage::CloudDatabase;
use crate::{OpenClipboard, HWND, EmptyClipboard, GlobalAlloc, GMEM_MOVEABLE, GlobalLock, GlobalUnlock, SetClipboardData, HANDLE, CloseClipboard};

pub struct App {
    history: Vec<ClipSummary>,
    db_path: String,
    cloud_db_path: String,
    current_page: i32,
    items_per_page: i32,
    total_count: i32,
    synced_hashes: HashSet<String>,
    visible: Arc<AtomicBool>,
    last_visible: bool,
    last_focused: bool,
    has_ever_focused: bool,
    needs_refresh: Arc<AtomicBool>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        visible: Arc<AtomicBool>,
        needs_refresh: Arc<AtomicBool>,
    ) -> Self {
        let _ = crate::EGUI_CTX.set(cc.egui_ctx.clone());

        let mut app = Self {
            history: Vec::new(),
            db_path: "clipboard.db".to_string(),
            cloud_db_path: "cloud.db".to_string(),
            current_page: 0,
            items_per_page: 20,
            total_count: 0,
            synced_hashes: HashSet::new(),
            visible,
            last_visible: true,
            last_focused: false,
            has_ever_focused: false,
            needs_refresh,
        };
        app.refresh_history();
        app
    }

    fn refresh_history(&mut self) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            self.total_count = db.get_total_count().unwrap_or(0);

            let max_pages = ((self.total_count as f32 / self.items_per_page as f32).ceil() as i32).max(1);
            if self.current_page >= max_pages {
                self.current_page = max_pages - 1;
            }

            let offset = self.current_page * self.items_per_page;
            if let Ok(clips) = db.get_latest_clips(self.items_per_page, offset) {
                self.history = clips;
            }
        }

        if let Ok(cloud) = CloudDatabase::new(&self.cloud_db_path, "pwd") {
            self.synced_hashes = cloud.get_synced_hashes().unwrap_or_default();
        }
    }

    fn clear_history(&mut self) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if db.clear_all_clips().is_ok() {
                self.current_page = 0;
                self.refresh_history();
            }
        }
    }

    fn delete_single(&mut self, hash: &str) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if db.delete_clip_by_hash(hash).is_ok() {
                self.refresh_history();
            }
        }
    }

    fn push_to_cloud(&mut self, hash: &str) {
        let source = match Database::new(&self.db_path, "pwd") {
            Ok(db) => db,
            Err(e) => { eprintln!("push_to_cloud: clipboard.db open failed: {}", e); return; }
        };
        let cloud = match CloudDatabase::new(&self.cloud_db_path, "pwd") {
            Ok(db) => db,
            Err(e) => { eprintln!("push_to_cloud: cloud.db open failed: {}", e); return; }
        };
        match cloud.copy_clip_from(hash, &source) {
            Ok(_) => {
                println!("Pushed {} to cloud.db", hash);
                self.synced_hashes.insert(hash.to_string());
            }
            Err(e) => eprintln!("push_to_cloud failed: {}", e),
        }
    }

    fn restore_clip(&self, hash: &str) {
        let db = match Database::new(&self.db_path, "pwd") {
            Ok(db) => db,
            Err(_) => return,
        };
        let payloads = match db.get_clip_payloads(hash) {
            Ok(p) => p,
            Err(_) => return,
        };
        unsafe {
            crate::set_restoring(true);
            if OpenClipboard(HWND(0)).is_ok() {
                let _ = EmptyClipboard();
                for payload in payloads {
                    if let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, payload.data.len()) {
                        let ptr = GlobalLock(hglobal);
                        if !ptr.is_null() {
                            std::ptr::copy_nonoverlapping(
                                payload.data.as_ptr(),
                                ptr as *mut u8,
                                payload.data.len(),
                            );
                            let _ = GlobalUnlock(hglobal);
                            let _ = SetClipboardData(payload.format_id, HANDLE(hglobal.0 as isize));
                        }
                    }
                }
                let _ = CloseClipboard();
                println!("Restored {}", hash);
            }
            crate::set_restoring(false);
        }
    }

    fn hide(&self) {
        self.visible.store(false, Ordering::Relaxed);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.needs_refresh.swap(false, Ordering::Relaxed) {
            self.refresh_history();
        }

        let cur_visible = self.visible.load(Ordering::Relaxed);
        if self.last_visible != cur_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(cur_visible));
            self.last_visible = cur_visible;
        }

        let focused = ctx.input(|i| i.focused);
        if focused {
            self.has_ever_focused = true;
        }
        if self.has_ever_focused && self.last_focused && !focused && !crate::is_restoring() {
            self.hide();
        }
        self.last_focused = focused;

        let mut restore_hash: Option<String> = None;
        let mut delete_hash: Option<String> = None;
        let mut cloud_hash: Option<String> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("History");
                if ui.button("Hide").clicked() {
                    self.hide();
                }
                if ui.button("Clear All").clicked() {
                    self.needs_refresh.store(true, Ordering::Relaxed);
                    self.clear_history();
                }
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .id_source("clip_scroll")
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    for clip in &self.history {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&clip.owner).strong());
                                ui.label(egui::RichText::new(&clip.fg_title).strong());
                                ui.label(&clip.timestamp);

                                if self.synced_hashes.contains(&clip.hash) {
                                    ui.label(egui::RichText::new("☁").color(egui::Color32::from_rgb(100, 160, 255)));
                                } else if ui.small_button("⬆ Cloud").clicked() {
                                    cloud_hash = Some(clip.hash.clone());
                                }
                            });

                            ui.label(&clip.preview);

                            ui.horizontal(|ui| {
                                if ui.button("Restore").clicked() {
                                    restore_hash = Some(clip.hash.clone());
                                }
                                if ui.button("Delete").clicked() {
                                    delete_hash = Some(clip.hash.clone());
                                }
                            });
                        });
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                let prev_enabled = self.current_page > 0;
                if ui.add_enabled(prev_enabled, egui::Button::new("prev")).clicked() {
                    self.current_page -= 1;
                    self.refresh_history();
                }

                let total_pages = ((self.total_count as f32 / self.items_per_page as f32).ceil() as i32).max(1);
                ui.label(format!(
                    "{} of {}",
                    if self.total_count == 0 { 0 } else { self.current_page + 1 },
                    if self.total_count == 0 { 0 } else { total_pages },
                ));

                let next_enabled = (self.current_page + 1) < total_pages && self.total_count > 0;
                if ui.add_enabled(next_enabled, egui::Button::new("next")).clicked() {
                    self.current_page += 1;
                    self.refresh_history();
                }

                ui.weak(format!("(total: {})", self.total_count));
            });
        });

        if let Some(hash) = restore_hash {
            self.restore_clip(&hash);
        }
        if let Some(hash) = delete_hash {
            self.needs_refresh.store(true, Ordering::Relaxed);
            self.delete_single(&hash);
        }
        if let Some(hash) = cloud_hash {
            self.push_to_cloud(&hash);
        }
    }
}