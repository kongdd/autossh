//! Windows system-tray integration.
//!
//! Tray callbacks may run while the native window is hidden. They therefore
//! wake egui explicitly after forwarding a small command to the app thread.

use std::sync::mpsc::{self, Receiver};

use crate::tunnel_icon;
use eframe::egui;
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

const MENU_SHOW: &str = "autossh-show";
const MENU_EXIT: &str = "autossh-exit";

#[derive(Clone, Copy, Debug)]
pub enum TrayCommand {
    Show,
    Exit,
}

pub struct WindowsTray {
    // The icon disappears when the final TrayIcon handle is dropped.
    _icon: TrayIcon,
    commands: Receiver<TrayCommand>,
}

impl WindowsTray {
    pub fn new(ctx: &egui::Context) -> anyhow::Result<Self> {
        let show = MenuItem::with_id(MENU_SHOW, "Open autossh-core", true, None);
        let separator = PredefinedMenuItem::separator();
        let exit = MenuItem::with_id(MENU_EXIT, "Exit", true, None);
        let menu = Menu::with_items(&[&show, &separator, &exit])?;

        let (sender, commands) = mpsc::channel();

        let menu_sender = sender.clone();
        let menu_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match event.id.as_ref() {
                MENU_SHOW => Some(TrayCommand::Show),
                MENU_EXIT => Some(TrayCommand::Exit),
                _ => None,
            };
            if let Some(command) = command {
                let _ = menu_sender.send(command);
                menu_ctx.request_repaint();
            }
        }));

        let tray_ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            // tray-icon 0.21/0.24 register the tray's window class without
            // `CS_DBLCLKS`, so `WM_LBUTTONDBLCLK` never arrives and the
            // `DoubleClick` variant is effectively dead. Show on the first
            // left-click Up instead — same behaviour QQ / WeChat use.
            let show_window = matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            );
            if show_window {
                let _ = sender.send(TrayCommand::Show);
                tray_ctx.request_repaint();
            }
        }));

        let icon = TrayIconBuilder::new()
            .with_tooltip("autossh-core")
            .with_icon(tunnel_icon::tray_icon()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;

        Ok(Self {
            _icon: icon,
            commands,
        })
    }

    pub fn try_recv(&self) -> Option<TrayCommand> {
        self.commands.try_recv().ok()
    }
}
