pub mod buffer_view;
use std::sync::Arc;

pub use buffer_view::*;

pub mod smooth_scroll;
use egui::{Pos2, Rect, Response, Vec2};
pub use smooth_scroll::*;

pub mod keymaps;
pub use keymaps::*;

use crate::MonitorSettings;

pub struct TerminalCalc {
    /// The height of the buffer in chars
    pub char_height: f32,

    /// The height of the visible area in chars
    pub buffer_char_height: f32,

    /// Size of a single terminal pixel in screen pixels
    pub scale: Vec2,

    pub char_size: Vec2,
    pub font_height: f32,
    pub first_line: f32,
    pub rect: egui::Rect,
    pub buffer_rect: egui::Rect,
    pub scrollbar_rect: egui::Rect,
    pub char_scroll_positon: f32,
}

pub fn show_terminal_area(
    ui: &mut egui::Ui,
    buffer_view: Arc<eframe::epaint::mutex::Mutex<BufferView>>,
    focus_lock: bool,
    filter: i32,
    settings: MonitorSettings,
    stick_to_bottom: bool,
) -> (Response, TerminalCalc) {
    let buf_h = buffer_view.lock().buf.get_buffer_height() as f32;
    let real_height = buffer_view.lock().buf.get_real_buffer_height() as f32;
    let buf_w = buffer_view.lock().buf.get_buffer_width() as f32;

    let font_dimensions = buffer_view.lock().buf.get_font_dimensions();

    SmoothScroll::new()
        .with_lock_focus(focus_lock)
        .with_stick_to_bottom(stick_to_bottom)
        .show(
            ui,
            |rect| {
                let size = rect.size();

                let mut scale_x = size.x / font_dimensions.width as f32 / buf_w;
                let mut scale_y = size.y / font_dimensions.height as f32 / buf_h;

                if scale_x < scale_y {
                    scale_y = scale_x;
                } else {
                    scale_x = scale_y;
                }

                let char_size = Vec2::new(
                    font_dimensions.width as f32 * scale_x,
                    font_dimensions.height as f32 * scale_y,
                );

                let rect_w = buf_w * char_size.x;
                let rect_h = buf_h * char_size.y;

                let buffer_rect = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + (rect.width() - rect_w) / 2.,
                        rect.top() + (rect.height() - rect_h) / 2.,
                    ),
                    Vec2::new(rect_w, rect_h),
                );
                // Set the scrolling height.
                TerminalCalc {
                    char_height: real_height,
                    buffer_char_height: buf_h,
                    scale: Vec2::new(scale_x, scale_y),
                    char_size: Vec2::new(
                        font_dimensions.width as f32 * scale_x,
                        font_dimensions.height as f32 * scale_y,
                    ),
                    font_height: font_dimensions.height as f32,
                    first_line: 0.,
                    rect,
                    buffer_rect,
                    scrollbar_rect: Rect::NOTHING,
                    char_scroll_positon: 0.,
                }
            },
            |ui, calc| {
                let viewport_top = calc.char_scroll_positon * calc.scale.y;
                calc.first_line = viewport_top / calc.char_size.y;

                {
                    let buffer_view = &mut buffer_view.lock();
                    buffer_view.char_size = calc.char_size;
                    if buffer_view.viewport_top != viewport_top {
                        buffer_view.viewport_top = viewport_top;
                        buffer_view.redraw_view();
                    }
                }

                let buffer_rect = calc.buffer_rect;
                let terminal_rect = calc.rect;
                let callback = egui::PaintCallback {
                    rect: terminal_rect,
                    callback: std::sync::Arc::new(egui_glow::CallbackFn::new(
                        move |info, painter| {
                            buffer_view.lock().render_contents(
                                painter.gl(),
                                &info,
                                buffer_rect,
                                terminal_rect,
                                filter,
                                &settings,
                            );
                        },
                    )),
                };
                ui.painter().add(callback);
            },
        )
}
