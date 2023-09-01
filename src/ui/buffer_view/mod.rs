
use egui::{Response, Vec2};
use glow::HasContext;
use icy_engine::{
    Buffer, BufferParser, CallbackAction, Caret, EngineResult, Position, Selection, Shape,
};

pub mod glerror;

use crate::{check_gl_error, MonitorSettings, TerminalCalc};

mod output_renderer;
mod sixel_renderer;
mod terminal_renderer;

#[derive(Clone, Copy)]
pub enum BufferInputMode {
    CP437,
    PETscii,
    ATAscii,
    ViewData,
}

pub struct Blink {
    is_on: bool,
    last_blink: u128,
    blink_rate: u128,
}

impl Blink {
    pub fn new(blink_rate: u128) -> Self {
        Self {
            is_on: false,
            last_blink: 0,
            blink_rate,
        }
    }

    pub fn is_on(&self) -> bool {
        self.is_on
    }

    pub fn update(&mut self, cur_ms: u128) -> bool {
        if cur_ms - self.last_blink > self.blink_rate {
            self.is_on = !self.is_on;
            self.last_blink = cur_ms;
            true
        } else {
            false
        }
    }
}

pub struct BufferView {
    pub buf: Buffer,
    pub caret: Caret,

    pub scale: f32,
    pub buffer_input_mode: BufferInputMode,
    pub viewport_top: f32,

    pub char_size: Vec2,

    pub button_pressed: bool,

    selection_opt: Option<Selection>,

    terminal_renderer: terminal_renderer::TerminalRenderer,
    sixel_renderer: sixel_renderer::SixelRenderer,
    output_renderer: output_renderer::OutputRenderer,

    drag_start: Option<Vec2>,
}

impl BufferView {
    pub fn new(gl: &glow::Context, filter: i32) -> Self {
        let mut buf = Buffer::create((80, 25));
        buf.is_terminal_buffer = true;

        BufferView::from_buffer(gl, buf, filter)
    }

    pub fn from_buffer(
        gl: &glow::Context,
        buf: Buffer,
        filter: i32,
    ) -> Self {
        let terminal_renderer = terminal_renderer::TerminalRenderer::new(gl);
        let sixel_renderer = sixel_renderer::SixelRenderer::new(gl, &buf, filter);
        let output_renderer =
            output_renderer::OutputRenderer::new(gl, &buf, filter);
        Self {
            buf,
            caret: Caret::default(),
            scale: 1.0,
            buffer_input_mode: BufferInputMode::CP437,
            button_pressed: false,
            viewport_top: 0.,
            selection_opt: None,
            char_size: Vec2::ZERO,
            terminal_renderer,
            sixel_renderer,
            output_renderer,
            drag_start: None,
        }
    }

    pub fn get_selection(&mut self) -> &mut Option<Selection> {
        &mut self.selection_opt
    }

    pub fn set_selection(&mut self, sel: Selection) {
        self.selection_opt = Some(sel);
    }

    pub fn clear_selection(&mut self) {
        self.selection_opt = None;
    }

    pub fn clear(&mut self) {
        self.caret.ff(&mut self.buf, 0);
    }

    pub fn get_copy_text(&mut self, buffer_parser: &dyn BufferParser) -> Option<String> {
        let Some(selection) = &self.selection_opt else {
            return None;
        };

        let mut res = String::new();
        if matches!(selection.shape, Shape::Rectangle) {
            let start = selection.min();
            let end = selection.max();
            for y in start.y..=end.y {
                for x in start.x..end.x {
                    let ch = self.buf.get_char(Position::new(x, y));
                    res.push(buffer_parser.convert_to_unicode(ch));
                }
                res.push('\n');
            }
        } else {
            let (start, end) = if selection.anchor.as_position() < selection.lead.as_position() {
                (selection.anchor.as_position(), selection.lead.as_position())
            } else {
                (selection.lead.as_position(), selection.anchor.as_position())
            };
            if start.y == end.y {
                for x in start.x..end.x {
                    let ch = self.buf.get_char(Position::new(x, start.y));
                    res.push(buffer_parser.convert_to_unicode(ch));
                }
            } else {
                for x in start.x..(self.buf.get_line_length(start.y as usize) as i32) {
                    let ch = self.buf.get_char(Position::new(x, start.y));
                    res.push(buffer_parser.convert_to_unicode(ch));
                }
                res.push('\n');
                for y in start.y + 1..end.y {
                    for x in 0..(self.buf.get_line_length(y as usize) as i32) {
                        let ch = self.buf.get_char(Position::new(x, y));
                        res.push(buffer_parser.convert_to_unicode(ch));
                    }
                    res.push('\n');
                }
                for x in 0..end.x {
                    let ch = self.buf.get_char(Position::new(x, end.y));
                    res.push(buffer_parser.convert_to_unicode(ch));
                }
            }
        }
        self.selection_opt = None;
        Some(res)
    }

    pub fn redraw_view(&mut self) {
        self.terminal_renderer.redraw_terminal();
    }

    pub fn redraw_palette(&mut self) {
        self.terminal_renderer.redraw_palette();
    }

    pub fn redraw_font(&mut self) {
        self.terminal_renderer.redraw_font();
    }

    pub fn print_char(
        &mut self,
        parser: &mut Box<dyn BufferParser>,
        c: char,
    ) -> EngineResult<CallbackAction> {
        let res = parser.print_char(&mut self.buf, 0, &mut self.caret, c);
        self.redraw_view();
        res
    }

    pub fn render_contents(
        &mut self,
        gl: &glow::Context,
        info: &egui::PaintCallbackInfo,
        buffer_rect: egui::Rect,
        terminal_rect: egui::Rect,
        scale_filter: i32,
        monitor_settings: &MonitorSettings,
        has_focus: bool,
    ) {
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            self.update_contents(gl, scale_filter);

            self.output_renderer.init_output(gl);
            self.terminal_renderer.render_terminal(
                gl,
                self,
                monitor_settings,
                has_focus,
            );
            // draw sixels
            let render_texture =
                self.sixel_renderer
                    .render_sixels(gl, self, &self.output_renderer);
            gl.enable(glow::SCISSOR_TEST);

            self.output_renderer.render_to_screen(
                gl,
                info,
                render_texture,
                buffer_rect,
                terminal_rect,
                monitor_settings,
            );
        }
        check_gl_error!(gl, "buffer_view.render_contents");
    }

    pub fn update_contents(
        &mut self,
        gl: &glow::Context,
        scale_filter: i32
    ) {
        self.sixel_renderer
            .update_sixels(gl, &mut self.buf, scale_filter);
        self.terminal_renderer.update_textures(
            gl,
            &mut self.buf,
            self.viewport_top,
            self.char_size,
        );
        self.output_renderer
            .update_render_buffer(gl, &self.buf, scale_filter);
        check_gl_error!(gl, "buffer_view.update_contents");
    }

    pub fn destroy(&self, gl: &glow::Context) {
        self.terminal_renderer.destroy(gl);
        self.output_renderer.destroy(gl);
        self.sixel_renderer.destroy(gl);
    }

    pub fn clear_buffer_screen(&mut self) {
        self.caret.set_background(0);
        self.buf.clear_screen(0, &mut self.caret);
        self.redraw_view();
    }

    pub fn set_buffer(&mut self, buf: Buffer) {
        self.buf = buf;
        self.redraw_font();
        self.redraw_palette();
    }

    pub fn handle_dragging(&mut self, response: Response, calc: TerminalCalc) {
        if response.drag_started() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                if calc.buffer_rect.contains(mouse_pos) {
                    self.drag_start = Some(calc.calc_click_pos(mouse_pos));
                }
            }
        }

        if response.drag_released() {
            self.drag_start = None;
        }
        //if response.dragged() {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
const SHADER_VERSION: &str = "#version 330";

#[cfg(target_arch = "wasm32")]
const SHADER_VERSION: &str = "#version 300 es";

#[macro_export]
macro_rules! prepare_shader {
    ($shader: expr) => {{
        format!("{}\n{}", $crate::ui::buffer_view::SHADER_VERSION, $shader)
    }};
}

const SHADER_SOURCE: &str = r#"precision highp float;

const float low  = -1.0;
const float high = 1.0;

void main() {
    vec2 vert = vec2(0, 0);
    switch (gl_VertexID) {
        case 0:
            vert = vec2(low, high);
            break;
        case 1:
            vert = vec2(high, high);
            break;
        case 2:
            vert = vec2(high, low);
            break;
        case 3:
            vert = vec2(low, high);
            break;
        case 4:
            vert = vec2(low, low);
            break;
        case 5:
            vert = vec2(high, low);
            break;
    }
    gl_Position = vec4(vert, 0.3, 1.0);
}
"#;
