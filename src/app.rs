use eframe::egui;
use crate::models::*;
use crate::storage::*;
use crate::{OpenClipboard, HWND, EmptyClipboard, GlobalAlloc, GMEM_MOVEABLE, GlobalLock, GlobalUnlock, SetClipboardData, HANDLE, CloseClipboard};

pub struct App {
    history: Vec<ClipSummary>,
    db_path: String,
    current_page: i32,
    items_per_page: i32,
    total_count: i32,
    visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    last_visible: bool,
}

impl App {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let _ = crate::EGUI_CTX.set(_cc.egui_ctx.clone());
        let mut app = Self {
            history: Vec::new(),
            db_path: "clipboard.db".to_string(),
            current_page: 0,
            items_per_page: 20,
            total_count: 0,
            visible,
            last_visible: true,
        };

        app.refresh_history();
        app
    }

    pub fn refresh_history(&mut self) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            self.total_count = db.get_total_count().unwrap_or(0);
            let max_pages = (self.total_count as f32 / self.items_per_page as f32).ceil() as i32;
            if self.current_page >= max_pages && max_pages > 0 {
                self.current_page = max_pages - 1;
            }
            let offset = self.current_page * self.items_per_page;
            if let Ok(latest) = db.get_latest_clips(self.items_per_page, offset) {
                self.history = latest;
            }
        }
    }

    pub fn clear_history(&mut self) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if let Ok(_) = db.clear_all_clips() {
                self.current_page = 0;
                self.refresh_history();
                println!("history cleared");
            }
        }
    }

    pub fn delete_single(&mut self, hash: &str) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if let Ok(_) = db.delete_clip_by_hash(hash) {
                self.refresh_history();
                println!("deleted clip {}", hash);
            }
        }
    }

    pub fn restore_clip(&mut self, hash: &str) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if let Ok(payloads) = db.get_clip_payloads(hash) {
                unsafe {
                    crate::set_restoring(true);
                    if OpenClipboard(HWND(0)).is_ok() {
                        let _ = EmptyClipboard();

                        for payload in payloads {
                            let hglobal = GlobalAlloc(GMEM_MOVEABLE, payload.data.len()).unwrap();
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
                        let _ = CloseClipboard();

                        println!("restored from {}", hash);
                    }
                    crate::set_restoring(false);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_history();
        let mut restore_hash: Option<String> = None;
        let mut delete_hash: Option<String> = None;
        let visible_clone = self.visible.clone();
        let cur = self.visible.load(std::sync::atomic::Ordering::Relaxed);
        if self.last_visible != cur {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(cur));
            self.last_visible = cur;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("History");
                if ui.button("Hide").clicked() {
                    visible_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                if ui.button("Clear All").clicked() {
                    self.clear_history();
                    self.refresh_history();
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
                        });
                        ui.label(&clip.preview);
                        if ui.button("Restore").clicked() {
                            restore_hash = Some(clip.hash.clone());
                        }
                        if ui.button("Delete").clicked() {
                            delete_hash = Some(clip.hash.clone());
                        }
                    });
                }
            });
            
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("prev").clicked() && self.current_page > 0 {
                    self.current_page -= 1;
                    self.refresh_history();
                }

                let total_pages = (self.total_count as f32 / self.items_per_page as f32).ceil() as i32;

                ui.label(format!(
                    "{} of {}",
                    if total_pages == 0 { 0 } else { self.current_page + 1 },
                    total_pages
                ));

                let has_next = (self.current_page + 1) < total_pages;
                if ui.add_enabled(has_next, egui::Button::new("next")).clicked() {
                    self.current_page += 1;
                    self.refresh_history();
                }

                ui.weak(format!("(history size: {})", self.total_count));
            });
        });

        if let Some(hash) = restore_hash {
            self.restore_clip(&hash);
        }

        if let Some(hash) = delete_hash {
            self.delete_single(&hash);
        }

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
