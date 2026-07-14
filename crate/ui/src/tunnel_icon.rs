//! Single source: `docs/autossh-tunnel.ico` (compile-time bytes + PE embed via `build.rs`).

use anyhow::Context;
use eframe::egui::IconData;
use image::imageops::FilterType;
use std::sync::OnceLock;
use tray_icon::Icon;

/// Keep in sync with `crate/ui/build.rs` icon path.
const TUNNEL_ICO: &[u8] = include_bytes!("../../../docs/autossh-tunnel.ico");

const WINDOW_ICON_PX: u32 = 64;
const TRAY_ICON_PX: u32 = 32;

static WINDOW_ICON: OnceLock<IconData> = OnceLock::new();
static TRAY_RGBA: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();

/// Title bar / Alt+Tab (via winit `with_icon`).
pub fn window_icon() -> Option<IconData> {
    Some(
        WINDOW_ICON
            .get_or_init(|| {
                let (rgba, width, height) =
                    rgba_at_size(WINDOW_ICON_PX).expect("decode autossh-tunnel.ico for window");
                IconData {
                    rgba,
                    width,
                    height,
                }
            })
            .clone(),
    )
}

/// Notification area; decoded from the same `.ico` as the window icon.
pub fn tray_icon() -> anyhow::Result<Icon> {
    let (rgba, width, height) = TRAY_RGBA.get_or_init(|| {
        rgba_at_size(TRAY_ICON_PX).expect("decode autossh-tunnel.ico for tray")
    });
    Icon::from_rgba(rgba.clone(), *width, *height).map_err(|error| anyhow::anyhow!("{error:?}"))
}

fn rgba_at_size(size: u32) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    let image = image::load_from_memory(TUNNEL_ICO).context("decode autossh-tunnel.ico")?;
    let image = image.resize_to_fill(size, size, FilterType::Triangle);
    let image = image.to_rgba8();
    Ok((image.into_raw(), size, size))
}