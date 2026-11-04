//! The system clipboard, read when a text box pastes. arboard is the plumbing: the
//! Windows clipboard, AppKit's pasteboard on macOS and X11 selections on Linux (Wayland
//! desktops through XWayland).

/// The clipboard's text now, or why there is none.
pub fn text() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|e| e.to_string())
}
