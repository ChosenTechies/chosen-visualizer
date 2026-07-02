use crate::settings::{Settings, TaskbarEdge};
use eframe::egui;

#[cfg(windows)]
pub type NativeWindowHandle = windows_sys::Win32::Foundation::HWND;
#[cfg(not(windows))]
pub type NativeWindowHandle = ();

#[derive(Clone, Copy, Debug)]
pub struct DisplayArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, PartialEq)]
pub struct WindowFlags {
    pub always_on_top: bool,
    pub click_through: bool,
    pub frameless: bool,
    pub desktop_widget: bool,
    pub desktop_only: bool,
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub desktop_width: i32,
    pub desktop_height: i32,
    pub taskbar_strip: bool,
    pub taskbar_edge: TaskbarEdge,
    pub strip_thickness: f32,
}

impl From<&Settings> for WindowFlags {
    fn from(settings: &Settings) -> Self {
        Self {
            always_on_top: settings.always_on_top,
            click_through: settings.click_through,
            frameless: settings.frameless,
            desktop_widget: settings.desktop_widget,
            desktop_only: settings.desktop_only,
            desktop_x: settings.desktop_x,
            desktop_y: settings.desktop_y,
            desktop_width: settings.desktop_width,
            desktop_height: settings.desktop_height,
            taskbar_strip: settings.taskbar_strip,
            taskbar_edge: settings.taskbar_edge,
            strip_thickness: settings.strip_thickness,
        }
    }
}

pub fn current_display_area(handle: Option<NativeWindowHandle>) -> Option<DisplayArea> {
    #[cfg(windows)]
    {
        handle.and_then(|hwnd| unsafe { windows_impl::current_display_area(hwnd) })
    }

    #[cfg(not(windows))]
    {
        let _ = handle;
        None
    }
}

pub fn apply_egui_viewport(ctx: &egui::Context, settings: &Settings) {
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        if settings.always_on_top && !settings.desktop_only {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        },
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!settings.frameless));
    if settings.desktop_widget {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            settings.desktop_width as f32,
            settings.desktop_height as f32,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            settings.desktop_x as f32,
            settings.desktop_y as f32,
        )));
    }
}

pub fn apply_native(handle: Option<NativeWindowHandle>, settings: &Settings) {
    #[cfg(windows)]
    if let Some(hwnd) = handle {
        unsafe {
            windows_impl::apply(hwnd, settings);
        }
    }

    #[cfg(not(windows))]
    let _ = (handle, settings);
}

#[cfg(not(windows))]
pub fn platform_note() -> Option<&'static str> {
    Some("Native desktop widget and taskbar strip placement are implemented for Windows.")
}

#[cfg(windows)]
pub fn platform_note() -> Option<&'static str> {
    None
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongW, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOPMOST,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowLongW, SetWindowPos,
            WS_EX_LAYERED, WS_EX_TRANSPARENT,
        },
    };

    pub unsafe fn current_display_area(hwnd: HWND) -> Option<DisplayArea> {
        let info = unsafe { monitor_info(hwnd)? };
        let work = info.rcWork;
        Some(DisplayArea {
            x: work.left,
            y: work.top,
            width: work.right - work.left,
            height: work.bottom - work.top,
        })
    }

    pub unsafe fn apply(hwnd: HWND, settings: &Settings) {
        if hwnd.is_null() {
            return;
        }

        let mut ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 };
        if settings.click_through {
            ex_style |= WS_EX_LAYERED | WS_EX_TRANSPARENT;
        } else {
            ex_style &= !WS_EX_TRANSPARENT;
        }
        unsafe { SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style as i32) };

        if settings.desktop_widget || settings.desktop_only {
            unsafe { position_desktop_widget(hwnd, settings) };
        } else if settings.taskbar_strip {
            unsafe {
                position_strip(
                    hwnd,
                    settings.taskbar_edge,
                    settings.strip_thickness,
                    settings.always_on_top,
                )
            };
        } else {
            unsafe {
                SetWindowPos(
                    hwnd,
                    if settings.always_on_top {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    },
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
    }

    unsafe fn position_desktop_widget(hwnd: HWND, settings: &Settings) {
        // "Desktop only" keeps the widget pinned to the bottom of the z-order so
        // it stays on the desktop and never overlays other applications.
        let pin_to_desktop = settings.desktop_only || !settings.always_on_top;
        let (x, y, w, h, flags) = if settings.desktop_widget {
            (
                settings.desktop_x,
                settings.desktop_y,
                settings.desktop_width,
                settings.desktop_height,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
        } else {
            // Desktop-only without widget placement: only adjust the z-order.
            (0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED)
        };
        unsafe {
            SetWindowPos(
                hwnd,
                if pin_to_desktop {
                    HWND_BOTTOM
                } else {
                    HWND_TOPMOST
                },
                x,
                y,
                w,
                h,
                flags,
            );
        }
    }

    unsafe fn position_strip(hwnd: HWND, edge: TaskbarEdge, thickness: f32, always_on_top: bool) {
        let Some(info) = (unsafe { monitor_info(hwnd) }) else {
            return;
        };

        let work = info.rcWork;
        let width = work.right - work.left;
        let height = work.bottom - work.top;
        let thickness = thickness.round() as i32;
        let (x, y, w, h) = match edge {
            TaskbarEdge::Bottom => (work.left, work.bottom - thickness, width, thickness),
            TaskbarEdge::Top => (work.left, work.top, width, thickness),
            TaskbarEdge::Left => (work.left, work.top, thickness, height),
            TaskbarEdge::Right => (work.right - thickness, work.top, thickness, height),
        };

        unsafe {
            SetWindowPos(
                hwnd,
                if always_on_top {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                },
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    unsafe fn monitor_info(hwnd: HWND) -> Option<MONITORINFO> {
        if hwnd.is_null() {
            return None;
        }

        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return None;
        }

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            None
        } else {
            Some(info)
        }
    }
}
