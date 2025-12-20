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
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            history: Vec::new(),
            db_path: "clipboard.db".to_string(),
            current_page: 0,
            items_per_page: 20,
            total_count: 0,
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

    pub fn restore_clip(&mut self, hash: &str) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if let Ok(payloads) = db.get_clip_payloads(hash) {
                unsafe {
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
                                GlobalUnlock(hglobal).expect("thread unlock issue");

                                let _ = SetClipboardData(payload.format_id, HANDLE(hglobal.0 as isize));
                            }
                        }
                        let _ = CloseClipboard();

                        println!("restored from {}", hash);
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut restore_hash: Option<String> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("History");
                if ui.button("Refresh").clicked() {
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

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
