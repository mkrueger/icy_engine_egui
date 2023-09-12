use std::path::PathBuf;

use egui::{Response, Vec2};
use glow::HasContext;
use icy_engine::{
    editor::EditState, Buffer, BufferParser, CallbackAction, Caret, EngineResult, Position,
    Selection, TextPane,
};

pub mod glerror;

use crate::{check_gl_error, TerminalCalc, TerminalOptions};

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
    edit_state: EditState,

    pub scale: f32,
    pub buffer_input_mode: BufferInputMode,
    pub viewport_top: f32,

    pub char_size: Vec2,

    pub calc: TerminalCalc,

    pub button_pressed: bool,

    terminal_renderer: terminal_renderer::TerminalRenderer,
    sixel_renderer: sixel_renderer::SixelRenderer,
    output_renderer: output_renderer::OutputRenderer,
    reference_image_path: Option<PathBuf>,
    drag_start: Option<Vec2>,
}

impl BufferView {
    pub fn new(gl: &glow::Context, filter: i32) -> Self {
        let mut buf = Buffer::create((80, 25));
        buf.is_terminal_buffer = true;

        BufferView::from_buffer(gl, buf, filter)
    }

    pub fn from_buffer(gl: &glow::Context, buf: Buffer, filter: i32) -> Self {
        let terminal_renderer = terminal_renderer::TerminalRenderer::new(gl);
        let calc = TerminalCalc::default();
        let sixel_renderer = sixel_renderer::SixelRenderer::new(gl, &buf, &calc, filter);
        let output_renderer = output_renderer::OutputRenderer::new(gl, &buf, &calc, filter);
        Self {
            edit_state: EditState::from_buffer(buf),
            scale: 1.0,
            buffer_input_mode: BufferInputMode::CP437,
            button_pressed: false,
            viewport_top: 0.,
            char_size: Vec2::ZERO,
            terminal_renderer,
            sixel_renderer,
            output_renderer,
            drag_start: None,
            reference_image_path: None,
            calc,
        }
    }

    pub fn set_parser(&mut self, parser: Box<dyn BufferParser>) {
        self.edit_state.set_parser(parser);
    }

    pub fn get_parser(&self) -> &dyn BufferParser {
        self.edit_state.get_parser()
    }

    pub fn get_width(&self) -> i32 {
        self.edit_state.get_buffer().get_width()
    }

    pub fn get_height(&self) -> i32 {
        self.edit_state.get_buffer().get_height()
    }

    pub fn get_edit_state(&self) -> &EditState {
        &self.edit_state
    }

    pub fn get_edit_state_mut(&mut self) -> &mut EditState {
        &mut self.edit_state
    }

    pub fn get_buffer(&self) -> &Buffer {
        self.edit_state.get_buffer()
    }

    pub fn get_buffer_mut(&mut self) -> &mut Buffer {
        self.edit_state.get_buffer_mut()
    }

    pub fn get_caret(&self) -> &Caret {
        self.edit_state.get_caret()
    }

    pub fn get_caret_mut(&mut self) -> &mut Caret {
        self.edit_state.get_caret_mut()
    }

    pub fn get_selection(&self) -> Option<Selection> {
        self.edit_state.get_selection()
    }

    pub fn set_selection(&mut self, sel: impl Into<Selection>) {
        let _ = self.edit_state.set_selection(sel.into());
    }

    pub fn clear_selection(&mut self) {
        let _ = self.edit_state.clear_selection();
    }

    pub fn clear(&mut self) {
        let cur_layer = self.edit_state.get_current_layer();
        self.get_buffer_mut().reset_terminal();
        self.get_buffer_mut().layers[cur_layer].clear();
        self.get_buffer_mut().stop_sixel_threads();

        self.get_caret_mut().set_position(Position::default());
        self.get_caret_mut().is_visible = true;
        self.get_caret_mut().reset_color_attribute();
    }

    pub fn get_copy_text(&mut self) -> Option<String> {
        self.edit_state.get_copy_text()
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

    pub fn print_char(&mut self, c: char) -> EngineResult<CallbackAction> {
        let edit_state = &mut self.edit_state;
        let (buf, caret, parser) = edit_state.get_buffer_and_caret_mut();
        parser.print_char(buf, 0, caret, c)
    }

    pub fn render_contents(
        &mut self,
        gl: &glow::Context,
        info: &egui::PaintCallbackInfo,
        options: &TerminalOptions,
    ) {
        let has_focus = self.calc.has_focus;

        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            self.update_contents(gl, options.filter);

            self.output_renderer.bind_framebuffers(gl);
            self.terminal_renderer
                .render_terminal(gl, self, &options.settings, has_focus);
            // draw sixels
            let render_texture = self
                .sixel_renderer
                .render_sixels(gl, self, &self.output_renderer);
            gl.enable(glow::SCISSOR_TEST);

            self.output_renderer.render_to_screen(
                gl,
                info,
                self,
                render_texture,
                self.output_renderer.render_data_texture,
                &self.calc,
                options,
            );
            self.output_renderer.unbind_framebuffers(gl);
        }
        check_gl_error!(gl, "buffer_view.render_contents");
    }

    pub fn update_contents(&mut self, gl: &glow::Context, scale_filter: i32) {
        let edit_state = &mut self.edit_state;
        self.sixel_renderer.update_sixels(
            gl,
            edit_state.get_buffer_mut(),
            &self.calc,
            scale_filter,
        );
        self.terminal_renderer.update_textures(
            gl,
            edit_state,
            &self.calc,
            self.viewport_top,
            self.char_size,
        );
        self.output_renderer.update_render_buffer(
            gl,
            edit_state.get_buffer_mut(),
            &self.calc,
            scale_filter,
        );

        check_gl_error!(gl, "buffer_view.update_contents");
    }

    pub fn destroy(&self, gl: &glow::Context) {
        self.terminal_renderer.destroy(gl);
        self.output_renderer.destroy(gl);
        self.sixel_renderer.destroy(gl);
    }

    pub fn clear_buffer_screen(&mut self) {
        self.get_caret_mut().set_background(0);
        self.clear();
        self.redraw_view();
    }

    pub fn set_buffer(&mut self, buf: Buffer) {
        self.edit_state.set_buffer(buf);
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

    pub fn get_reference_image_path(&self) -> Option<PathBuf> {
        self.reference_image_path.clone()
    }

    pub fn load_reference_image(&mut self, path: &std::path::Path) {
        if let Ok(image) = image::open(path) {
            self.reference_image_path = Some(path.to_path_buf());
            let image = image.to_rgba8();
            self.terminal_renderer.reference_image = Some(image);
            self.terminal_renderer.show_reference_image = true;
            self.terminal_renderer.load_reference_image = true;
        }
    }

    pub fn clear_reference_image(&mut self) {
        self.terminal_renderer.reference_image = None;
        self.terminal_renderer.show_reference_image = false;
    }

    pub fn toggle_reference_image(&mut self) {
        self.terminal_renderer.show_reference_image = !self.terminal_renderer.show_reference_image;
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
