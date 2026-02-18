pub struct ClipboardPayload {
    pub format_id: u32,
    pub format_name: String,
    pub data: Vec<u8>,
}

pub struct ClipboardMsg {
    pub owner: String,
    pub fg_title: String,
    pub exe_path: String,
    pub hash: String,
    pub payloads: Vec<ClipboardPayload>,
}

pub struct ClipSummary {
    pub timestamp: String,
    pub owner: String,
    pub fg_title: String,
    pub preview: String,
    pub hash: String,
    pub is_image: bool,
}