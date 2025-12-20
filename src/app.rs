use eframe::egui;
use crate::models::*;
use crate::storage::*;
use crate::{OpenClipboard, HWND, EmptyClipboard, GlobalAlloc, GMEM_MOVEABLE, GlobalLock, GlobalUnlock, SetClipboardData, HANDLE, CloseClipboard};

pub struct App {
    history: Vec<ClipSummary>,
    db_path: String,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            history: Vec::new(),
            db_path: "clipboard.db".to_string(),
        };

        app.refresh_history();
        app
    }

    pub fn refresh_history(&mut self) {
        if let Ok(db) = Database::new(&self.db_path, "pwd") {
            if let Ok(latest) = db.get_latest_clips(20) {
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
            ui.heading("History");
            if ui.button("Refresh").clicked() {
                self.refresh_history();
            }

            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
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
        });

        if let Some(hash) = restore_hash {
            self.restore_clip(&hash);
        }

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
