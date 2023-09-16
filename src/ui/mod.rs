pub mod buffer_view;
use std::sync::Arc;

pub use buffer_view::*;

pub mod smooth_scroll;
use egui::{Pos2, Rect, Response, Vec2};
use icy_engine::TextPane;
pub use smooth_scroll::*;

pub mod keymaps;
pub use keymaps::*;

use crate::MonitorSettings;

#[derive(Clone, Copy)]
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
    pub terminal_rect: egui::Rect,
    pub buffer_rect: egui::Rect,
    pub scrollbar_rect: egui::Rect,
    pub char_scroll_positon: f32,
    pub forced_height: i32,
    pub real_height: i32,

    /// remainder for scaled mode
    pub scroll_remainder: f32,

    pub set_scroll_position_set_by_user: bool,

    pub has_focus: bool,
}

impl Default for TerminalCalc {
    fn default() -> Self {
        Self {
            char_height: Default::default(),
            buffer_char_height: Default::default(),
            scale: Default::default(),
            char_size: Default::default(),
            font_height: Default::default(),
            first_line: Default::default(),
            terminal_rect: egui::Rect::NOTHING,
            buffer_rect: egui::Rect::NOTHING,
            scrollbar_rect: egui::Rect::NOTHING,
            char_scroll_positon: Default::default(),
            forced_height: Default::default(),
            scroll_remainder: Default::default(),
            set_scroll_position_set_by_user: Default::default(),
            has_focus: Default::default(),
            real_height: 0,
        }
    }
}

impl TerminalCalc {
    /// Returns the char position of the cursor in the buffer
    pub fn calc_click_pos(&self, click_pos: Pos2) -> Vec2 {
        (click_pos.to_vec2() - self.buffer_rect.left_top().to_vec2()) / self.char_size
            + Vec2::new(0.0, self.first_line)
    }
}

pub struct TerminalOptions {
    pub focus_lock: bool,
    pub filter: i32,
    pub settings: MonitorSettings,
    pub stick_to_bottom: bool,
    pub scale: Option<Vec2>,
    pub fit_width: bool,
    pub render_real_height: bool,
    pub use_terminal_height: bool,
    pub scroll_offset: Option<f32>,
    pub id: Option<egui::Id>,

    pub guide: Option<Vec2>,
    pub raster: Option<Vec2>,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            focus_lock: Default::default(),
            filter: glow::NEAREST as i32,
            settings: Default::default(),
            stick_to_bottom: Default::default(),
            scale: Default::default(),
            fit_width: false,
            render_real_height: false,
            use_terminal_height: true,
            scroll_offset: None,
            id: None,
            guide: None,
            raster: None,
        }
    }
}

pub fn show_terminal_area(
    ui: &mut egui::Ui,
    buffer_view: Arc<eframe::epaint::mutex::Mutex<BufferView>>,
    options: TerminalOptions,
) -> (Response, TerminalCalc) {
    let mut forced_height = buffer_view.lock().get_buffer().get_height();
    let mut buf_h = forced_height as f32;
    let real_height = if options.use_terminal_height {
        buffer_view
            .lock()
            .get_buffer()
            .get_line_count()
            .max(forced_height)
    } else {
        forced_height
    };
    let buf_w = buffer_view.lock().get_buffer().get_width() as f32;

    let font_dimensions = buffer_view.lock().get_buffer().get_font_dimensions();
    let buffer_view2: Arc<egui::mutex::Mutex<BufferView>> = buffer_view.clone();

    let mut scroll = SmoothScroll::new()
        .with_lock_focus(options.focus_lock)
        .with_stick_to_bottom(options.stick_to_bottom)
        .with_scroll_offset(options.scroll_offset);

    if let Some(id) = options.id {
        scroll = scroll.with_id(id);
    }

    let r = scroll.show(
        ui,
        options,
        |rect, options: &TerminalOptions| {
            let size = rect.size();

            let font_width = font_dimensions.width as f32
                + if buffer_view2.lock().get_buffer().use_letter_spacing() {
                    1.0
                } else {
                    0.0
                };

            let mut scale_x = size.x / font_width / buf_w;
            let mut scale_y = size.y / font_dimensions.height as f32 / buf_h;
            let mut scroll_remainder = 0.0;

            if options.fit_width {
                scale_y = scale_x;
            } else {
                if scale_x < scale_y {
                    scale_y = scale_x;
                } else {
                    scale_x = scale_y;
                }

                if let Some(scale) = options.scale {
                    scale_x = scale.x;
                    scale_y = scale.y;

                    let h = size.y / (font_dimensions.height as f32 * scale_y);
                    buf_h = h.ceil().min(real_height as f32);

                    if real_height as f32 > buf_h {
                        // HACK: for cutting the last line in scaled mode - not sure where the real rounding error is.
                        scroll_remainder = 1.0 - h.fract();
                    }

                    forced_height = (buf_h as i32).min(real_height);
                    buffer_view2.lock().redraw_view();
                }
            }

            let char_size = Vec2::new(
                font_width * scale_x,
                font_dimensions.height as f32 * scale_y,
            );

            let rect_w = buf_w * char_size.x;
            let rect_h = buf_h * char_size.y;
            let buffer_rect = Rect::from_min_size(
                Pos2::new(
                    (rect.left() + (rect.width() - rect_w) / 2.).floor(),
                    rect.top() + ((rect.height() - rect_h) / 2.).max(0.0).floor(),
                ),
                Vec2::new(rect_w.floor(), rect_h.floor()),
            );

            // Set the scrolling height.
            TerminalCalc {
                char_height: real_height as f32,
                buffer_char_height: buf_h,
                scale: Vec2::new(scale_x, scale_y),
                char_size: Vec2::new(
                    font_width * scale_x,
                    font_dimensions.height as f32 * scale_y,
                ),
                font_height: font_dimensions.height as f32,
                first_line: 0.,
                terminal_rect: rect,
                buffer_rect,
                scrollbar_rect: Rect::NOTHING,
                char_scroll_positon: 0.,
                set_scroll_position_set_by_user: false,
                forced_height,
                scroll_remainder,
                real_height,
                has_focus: false,
            }
        },
        |ui, calc, options: TerminalOptions| {
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
            buffer_view.lock().calc = *calc;
            let callback = egui::PaintCallback {
                rect: calc.terminal_rect,
                callback: std::sync::Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                    buffer_view
                        .lock()
                        .render_contents(painter.gl(), &info, &options);
                })),
            };
            ui.painter().add(callback);
        },
    );
    r
}
