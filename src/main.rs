mod storage;
mod models;
mod app;

use storage::Database;
use models::{ClipboardPayload, ClipboardMsg};
use app::App;

use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::System::LibraryLoader::*,
    Win32::UI::WindowsAndMessaging::*,
    Win32::System::Threading::*,
    Win32::System::ProcessStatus::*,
    Win32::System::DataExchange::*,
    Win32::System::Memory::*,
};

use std::sync::OnceLock;
use std::sync::mpsc::{channel, Sender};
use std::thread;

static TX: OnceLock<Sender<ClipboardMsg>> = OnceLock::new();

unsafe fn get_clipboard_source() -> String {
    let owner_hwnd = GetClipboardOwner();
    if owner_hwnd.0 == 0 { return "Unknown".to_string(); }
    let mut pid = 0u32;
    GetWindowThreadProcessId(owner_hwnd, Some(&mut pid));
    let process_handle = OpenProcess(
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
        false,
        pid,
    );
    if let Ok(handle) = process_handle {
        let mut buffer = [0u16; 260];
        let len = GetModuleBaseNameW(handle, None, &mut buffer);
        let _ = CloseHandle(handle);
        if len > 0 {
            return String::from_utf16_lossy(&buffer[..len as usize]);
        }
    }
    "Unknown Process".to_string()
}

unsafe fn process_clipboard_update(hwnd: HWND) {
    if OpenClipboard(hwnd).is_err() { return; }

    let source_app = get_clipboard_source();
    
    let mut title_buffer = [0u16; 512];
    let fg_hwnd = GetForegroundWindow();
    let len = GetWindowTextW(fg_hwnd, &mut title_buffer);
    let fg_title = String::from_utf16_lossy(&title_buffer[..len as usize]);

    let mut payloads = Vec::new();
    let mut format = EnumClipboardFormats(0);

    while format != 0 {
        if let Ok(handle) = GetClipboardData(format) {
            let hglobal = HGLOBAL(handle.0 as *mut _);
            let size = GlobalSize(hglobal);
            let ptr = GlobalLock(hglobal);

            if !ptr.is_null() && size > 0 {
                let slice = std::slice::from_raw_parts(ptr as *const u8, size);
                let data = slice.to_vec();

                let mut name_buf = [0u16; 256];
                let name_len = GetClipboardFormatNameW(format, &mut name_buf);
                let format_name = if name_len > 0 {
                    String::from_utf16_lossy(&name_buf[..name_len as usize])
                } else {
                    match format {
                        1 => "CF_TEXT".to_string(),
                        2 => "CF_BITMAP".to_string(),
                        13 => "CF_UNICODETEXT".to_string(),
                        15 => "CF_HDROP".to_string(),
                        _ => format!("ID_{}", format),
                    }
                };

                payloads.push(ClipboardPayload {
                    format_id: format,
                    format_name,
                    data,
                });

                let _ = GlobalUnlock(hglobal);
            }
        }
        format = EnumClipboardFormats(format);
    }

    let _ = CloseClipboard();

    if let Some(primary) = payloads.get(0) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        primary.data.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());

        let msg = ClipboardMsg {
            owner: source_app,
            fg_title: fg_title,
            exe_path: "UnknownPath".to_string(),
            hash,
            payloads,
        };

        if let Some(tx) = TX.get() {
            let _ = tx.send(msg);
        }
    }
}

const CLASS_NAME: &str = "OpenClipHiddenWindow";

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            process_clipboard_update(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() -> Result<()> {
    let (tx, rx) = channel::<ClipboardMsg>();
    TX.set(tx).map_err(|_| Error::from(HRESULT(0))).unwrap();

    thread::spawn(move || {
        let db = Database::new("clipboard.db", "pwd").expect("Failed to init DB");
        
        while let Ok(msg) = rx.recv() {
            let _ = db.save_snapshot(
                &msg.owner,
                &msg.fg_title,
                &msg.exe_path,
                &msg.hash,
                msg.payloads,
            );
            println!("Saved clip from: {}", msg.owner);
        }
    });

    thread::spawn(|| {
        unsafe {
            let hinstance = GetModuleHandleW(None).expect("Failed gmhw");
            let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                lpfnWndProc: Some(wnd_proc),
                ..Default::default()
            };

            RegisterClassExW(&wc);

            let _hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                wc.lpszClassName,
                PCWSTR(class_name.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                0, 0, 0, 0,
                HWND_MESSAGE,
                None,
                hinstance,
                None,
            );

            AddClipboardFormatListener(_hwnd).expect("Failed");

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Clip",
        native_options,
        Box::new(|cc| Box::new(App::new(cc))),
    ).expect("eframe failure");

    Ok(())
}