#![allow(clippy::float_cmp)]
use std::cmp::max;

use egui::epaint::ahash::HashMap;
use egui::Vec2;
use glow::HasContext as _;
use icy_engine::editor::EditState;
use icy_engine::Buffer;
use icy_engine::TextPane;
use image::EncodableLayout;
use image::RgbaImage;
use web_time::Instant;

use crate::prepare_shader;
use crate::MonitorSettings;
use crate::TerminalCalc;

use super::Blink;
use super::BufferView;

const FONT_TEXTURE_SLOT: u32 = 6;
const PALETTE_TEXTURE_SLOT: u32 = 8;
const BUFFER_TEXTURE_SLOT: u32 = 10;
const REFERENCE_IMAGE_TEXTURE_SLOT: u32 = 12;

pub struct TerminalRenderer {
    terminal_shader: glow::Program,

    font_lookup_table: HashMap<usize, usize>,

    terminal_render_texture: glow::Texture,
    font_texture: glow::Texture,
    palette_texture: glow::Texture,
    vertex_array: glow::VertexArray,

    // used to determine if palette needs to be updated - Note: palette only grows.
    old_palette_color_count: usize,

    redraw_view: bool,
    redraw_palette: bool,
    redraw_font: bool,

    caret_blink: Blink,
    character_blink: Blink,

    start_time: Instant,

    reference_image_texture: glow::Texture,
    pub reference_image: Option<RgbaImage>,
    pub load_reference_image: bool,
    pub show_reference_image: bool,
}

impl TerminalRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let reference_image_texture = create_reference_image_texture(gl);
            let font_texture = create_font_texture(gl);
            let palette_texture = create_palette_texture(gl);
            let terminal_render_texture = create_buffer_texture(gl);
            let terminal_shader = compile_shader(gl);

            let vertex_array = gl
                .create_vertex_array()
                .expect("Cannot create vertex array");

            Self {
                terminal_shader,
                font_lookup_table: HashMap::default(),
                old_palette_color_count: 0,

                terminal_render_texture,
                font_texture,
                palette_texture,
                reference_image: None,
                load_reference_image: false,
                show_reference_image: false,
                redraw_view: true,
                redraw_palette: true,
                redraw_font: true,
                vertex_array,
                caret_blink: Blink::new((1000.0 / 1.875) as u128 / 2),
                character_blink: Blink::new((1000.0 / 1.8) as u128),
                reference_image_texture,
                start_time: Instant::now(),
            }
        }
    }

    pub(crate) fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.vertex_array);

            gl.delete_program(self.terminal_shader);

            gl.delete_texture(self.terminal_render_texture);
            gl.delete_texture(self.font_texture);
            gl.delete_texture(self.palette_texture);
            gl.delete_texture(self.reference_image_texture);
        }
    }

    pub fn redraw_terminal(&mut self) {
        self.redraw_view = true;
    }

    pub fn redraw_palette(&mut self) {
        self.redraw_palette = true;
    }

    pub fn redraw_font(&mut self) {
        self.redraw_font = true;
    }

    pub fn update_textures(
        &mut self,
        gl: &glow::Context,
        edit_state: &mut EditState,
        calc: &TerminalCalc,
        viewport_top: f32,
        char_size: Vec2,
        use_fg: bool,
        use_bg: bool,
    ) {
        self.check_blink_timers();

        if self.redraw_font || edit_state.get_buffer().is_font_table_updated() {
            self.redraw_font = false;
            edit_state.get_buffer_mut().set_font_table_is_updated();
            self.update_font_texture(gl, edit_state.get_buffer());
        }

        if self.redraw_view {
            self.redraw_view = false;
            self.update_terminal_texture(
                gl,
                edit_state,
                calc,
                viewport_top,
                char_size,
                use_fg,
                use_bg,
            );
        }

        if self.redraw_palette
            || self.old_palette_color_count != edit_state.get_buffer().palette.colors.len()
        {
            self.redraw_palette = false;
            self.old_palette_color_count = edit_state.get_buffer().palette.colors.len();
            self.update_palette_texture(gl, edit_state.get_buffer());
        }

        if self.load_reference_image {
            if let Some(image) = &self.reference_image {
                self.update_reference_image_texture(gl, image);
            }
            self.load_reference_image = false;
        }
    }

    // Redraw whole terminal on caret or character blink update.
    fn check_blink_timers(&mut self) {
        let start: Instant = Instant::now();
        let since_the_epoch = start.duration_since(self.start_time.to_owned());
        let cur_ms = since_the_epoch.as_millis();
        if self.caret_blink.update(cur_ms) || self.character_blink.update(cur_ms) {
            self.redraw_terminal();
        }
    }

    fn update_font_texture(&mut self, gl: &glow::Context, buf: &Buffer) {
        let size = buf.get_font(0).unwrap().size;

        let w_ext = if buf.use_letter_spacing() { 1 } else { 0 };

        let w = size.width;
        let h = size.height;

        let mut font_data = Vec::new();
        let chars_in_line = 16;
        let line_width = (w + w_ext) * chars_in_line * 4;
        let height = h * 256 / chars_in_line;
        self.font_lookup_table.clear();
        font_data.resize((line_width * height) as usize * buf.font_count(), 0);

        for (cur_font_num, font) in buf.font_iter().enumerate() {
            self.font_lookup_table.insert(*font.0, cur_font_num);
            let fontpage_start = cur_font_num as i32 * (line_width * height);
            for ch in 0..256 {
                let cur_font = font.1;
                let glyph = cur_font
                    .get_glyph(unsafe { char::from_u32_unchecked(ch as u32) })
                    .unwrap();

                let x = ch % chars_in_line;
                let y = ch / chars_in_line;

                let offset = x * (w + w_ext) * 4 + y * h * line_width + fontpage_start;
                let last_scan_line = h.min(cur_font.size.height);
                for y in 0..last_scan_line {
                    if let Some(scan_line) = glyph.data.get(y as usize) {
                        let mut po = (offset + y * line_width) as usize;

                        for x in 0..w {
                            if scan_line & (128 >> x) == 0 {
                                po += 4;
                            } else {
                                // unroll
                                font_data[po] = 0xFF;
                                po += 1;
                                font_data[po] = 0xFF;
                                po += 1;
                                font_data[po] = 0xFF;
                                po += 1;
                                font_data[po] = 0xFF;
                                po += 1;
                            }
                        }
                        if buf.use_letter_spacing()
                            && (0xC0..=0xDF).contains(&ch)
                            && (scan_line & 1) != 0
                        {
                            // unroll
                            font_data[po] = 0xFF;
                            po += 1;
                            font_data[po] = 0xFF;
                            po += 1;
                            font_data[po] = 0xFF;
                            po += 1;
                            font_data[po] = 0xFF;
                        }
                    } else {
                        log::error!("error in font {} can't get line {y}", font.0);
                        font_data.extend(vec![0xFF; (w as usize) * 4]);
                    }
                }
            }
        }

        unsafe {
            gl.active_texture(glow::TEXTURE0 + FONT_TEXTURE_SLOT);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.font_texture));
            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                glow::RGBA as i32,
                line_width / 4,
                height,
                buf.font_count() as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&font_data),
            );
        }
    }

    fn update_reference_image_texture(&self, gl: &glow::Context, image: &RgbaImage) {
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.reference_image_texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                image.width() as i32,
                image.height() as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(image.as_bytes()),
            );
            crate::check_gl_error!(gl, "update_reference_image_texture");
        }
    }

    fn update_palette_texture(&self, gl: &glow::Context, buf: &Buffer) {
        let mut palette_data = Vec::new();
        for i in 0..buf.palette.colors.len() {
            let (r, g, b) = buf.palette.colors[i].get_rgb();
            palette_data.push(r);
            palette_data.push(g);
            palette_data.push(b);
            palette_data.push(0xFF);
        }
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.palette_texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                i32::try_from(buf.palette.colors.len()).unwrap(),
                1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&palette_data),
            );
            crate::check_gl_error!(gl, "update_palette_texture");
        }
    }

    fn update_terminal_texture(
        &self,
        gl: &glow::Context,
        edit_state: &EditState,
        calc: &TerminalCalc,
        viewport_top: f32,
        char_size: Vec2,
        use_fg: bool,
        use_bg: bool,
    ) {
        let buf = edit_state.get_buffer();
        let first_line = (viewport_top / char_size.y) as i32;
        let real_height = buf.get_height();
        let buf_h = calc.forced_height;

        let max_lines = max(0, real_height - buf_h) as i32;
        let scroll_back_line = max(0, max_lines - first_line);
        let first_line = 0.max(buf.get_height().saturating_sub(calc.forced_height));
        let mut buffer_data = Vec::with_capacity((2 * buf.get_width() * 4 * buf_h) as usize);
        let colors = buf.palette.colors.len() as u32 - 1;
        let mut y: i32 = 0;
        while y <= buf_h {
            let mut is_double_height = false;

            for x in 0..buf.get_width() {
                let mut ch = buf.get_char((x, first_line - scroll_back_line + y));
                if ch.attribute.is_double_height() {
                    is_double_height = true;
                }
                if ch.attribute.is_concealed() {
                    buffer_data.push(b' ');
                } else {
                    buffer_data.push(ch.ch as u8);
                }
                if !use_fg {
                    ch.attribute.set_foreground(7);
                    ch.attribute.set_is_bold(false);
                }
                let fg = if ch.attribute.is_bold() && ch.attribute.get_foreground() < 8 {
                    conv_color(ch.attribute.get_foreground() + 8, colors)
                } else {
                    conv_color(ch.attribute.get_foreground(), colors)
                };
                buffer_data.push(fg);

                if !use_bg {
                    ch.attribute.set_background(0);
                }
                let bg = conv_color(ch.attribute.get_background(), colors);
                buffer_data.push(bg);
                if buf.has_fonts() {
                    if let Some(font_number) = self.font_lookup_table.get(&ch.get_font_page()) {
                        buffer_data.push(*font_number as u8);
                    } else {
                        buffer_data.push(0);
                    }
                } else {
                    buffer_data.push(0);
                }
            }

            if is_double_height {
                for x in 0..buf.get_width() {
                    let ch = buf.get_char((x, first_line - scroll_back_line + y));

                    if ch.attribute.is_double_height() {
                        buffer_data.push(ch.ch as u8);
                    } else {
                        buffer_data.push(b' ');
                    }

                    if ch.attribute.is_bold() {
                        buffer_data.push(conv_color(ch.attribute.get_foreground() + 8, colors));
                    } else {
                        buffer_data.push(conv_color(ch.attribute.get_foreground(), colors));
                    }

                    buffer_data.push(conv_color(ch.attribute.get_background(), colors));

                    if buf.has_fonts() {
                        if let Some(font_number) = self.font_lookup_table.get(&ch.get_font_page()) {
                            buffer_data.push(*font_number as u8);
                        } else {
                            buffer_data.push(0);
                        }
                    } else {
                        buffer_data.push(0);
                    }
                }
            }

            if is_double_height {
                y += 2;
            } else {
                y += 1;
            }
        }

        y = 0;
        while y <= buf_h {
            let mut is_double_height = false;

            for x in 0..buf.get_width() {
                let ch = buf.get_char((x, first_line - scroll_back_line + y));
                let is_selected =
                    edit_state.get_is_mask_selected((x, first_line - scroll_back_line + y));

                let mut attr = if ch.attribute.is_double_underlined() {
                    3
                } else {
                    u8::from(ch.attribute.is_underlined())
                };
                if ch.attribute.is_crossed_out() {
                    attr |= 4;
                }

                if ch.attribute.is_double_height() {
                    is_double_height = true;
                    attr |= 8;
                }

                buffer_data.push(attr);
                buffer_data.push(attr);
                buffer_data.push(if is_selected { 255 } else { 0 });
                if !ch.is_visible() {
                    buffer_data.push(128);
                } else {
                    buffer_data.push(if ch.attribute.is_blinking() { 255 } else { 0 });
                }
            }

            if is_double_height {
                for x in 0..buf.get_width() {
                    let ch = buf.get_char((x, first_line - scroll_back_line + y));
                    let is_selected =
                        edit_state.get_is_selected((x, first_line - scroll_back_line + y));
                    let mut attr = if ch.attribute.is_double_underlined() {
                        3
                    } else {
                        u8::from(ch.attribute.is_underlined())
                    };
                    if ch.attribute.is_crossed_out() {
                        attr |= 4;
                    }

                    if ch.attribute.is_double_height() {
                        is_double_height = true;
                        attr |= 8;
                        attr |= 16;
                    }

                    buffer_data.push(attr);
                    buffer_data.push(attr);
                    buffer_data.push(if is_selected { 255 } else { 0 });
                    if !ch.is_visible() {
                        buffer_data.push(128);
                    } else {
                        buffer_data.push(if ch.attribute.is_blinking() { 255 } else { 0 });
                    }
                }
            }

            if is_double_height {
                y += 2;
            } else {
                y += 1;
            }
        }

        unsafe {
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.terminal_render_texture));
            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                glow::RGBA as i32,
                buf.get_width(),
                buf_h + 1,
                2,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&buffer_data),
            );
            crate::check_gl_error!(gl, "update_terminal_texture");
        }
    }

    pub(crate) fn render_terminal(
        &self,
        gl: &glow::Context,
        view_state: &BufferView,
        monitor_settings: &MonitorSettings,
        has_focus: bool,
    ) {
        unsafe {
            gl.active_texture(glow::TEXTURE0 + FONT_TEXTURE_SLOT);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.font_texture));

            gl.active_texture(glow::TEXTURE0 + PALETTE_TEXTURE_SLOT);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.palette_texture));

            gl.active_texture(glow::TEXTURE0 + BUFFER_TEXTURE_SLOT);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.terminal_render_texture));

            gl.active_texture(glow::TEXTURE0 + REFERENCE_IMAGE_TEXTURE_SLOT);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.reference_image_texture));

            self.run_shader(
                gl,
                view_state,
                view_state.output_renderer.render_buffer_size,
                monitor_settings,
                has_focus,
            );

            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_buffers(&[glow::COLOR_ATTACHMENT0, glow::COLOR_ATTACHMENT1]);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            crate::check_gl_error!(gl, "render_terminal");
        }
    }

    unsafe fn run_shader(
        &self,
        gl: &glow::Context,
        buffer_view: &BufferView,
        render_buffer_size: egui::Vec2,
        monitor_settings: &MonitorSettings,
        has_focus: bool,
    ) {
        let fontdim = buffer_view.get_buffer().get_font_dimensions();
        let fh = fontdim.height as f32;
        gl.bind_frag_data_location(self.terminal_shader, 0, "color1");
        gl.bind_frag_data_location(self.terminal_shader, 1, "color2");
        gl.use_program(Some(self.terminal_shader));
        gl.uniform_2_f32(
            gl.get_uniform_location(self.terminal_shader, "u_resolution")
                .as_ref(),
            render_buffer_size.x,
            render_buffer_size.y,
        );

        gl.uniform_2_f32(
            gl.get_uniform_location(self.terminal_shader, "u_output_resolution")
                .as_ref(),
            render_buffer_size.x,
            render_buffer_size.y + fh,
        );
        let top_pos = buffer_view.viewport_top.floor();
        let scroll_offset = (top_pos / buffer_view.char_size.y * fh) % fh;

        gl.uniform_2_f32(
            gl.get_uniform_location(self.terminal_shader, "u_position")
                .as_ref(),
            0.0,
            scroll_offset - fh,
        );
        let font_width = fontdim.width as f32
            + if buffer_view.get_buffer().use_letter_spacing() {
                1.0
            } else {
                0.0
            };
        let mut caret_pos = buffer_view.get_caret().get_position();
        if let Some(layer) = buffer_view.edit_state.get_cur_layer() {
            caret_pos += layer.get_offset();
        }

        let caret_x = caret_pos.x as f32 * font_width;

        let caret_h = if buffer_view.get_caret().insert_mode {
            fontdim.height as f32 / 2.0
        } else {
            2.0
        };

        let caret_y = caret_pos.y as f32 * fontdim.height as f32 + fontdim.height as f32
            - caret_h
            - (top_pos / buffer_view.char_size.y * fh)
            + scroll_offset;
        let caret_w = if self.caret_blink.is_on() && buffer_view.get_caret().is_visible && has_focus
        {
            font_width
        } else {
            0.0
        };
        gl.uniform_4_f32(
            gl.get_uniform_location(self.terminal_shader, "u_caret_rectangle")
                .as_ref(),
            caret_x / render_buffer_size.x,
            caret_y / (render_buffer_size.y + fh),
            (caret_x + caret_w) / render_buffer_size.x,
            (caret_y + caret_h) / (render_buffer_size.y + fh),
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.terminal_shader, "u_character_blink")
                .as_ref(),
            if self.character_blink.is_on() {
                1.0
            } else {
                0.0
            },
        );

        gl.uniform_2_f32(
            gl.get_uniform_location(self.terminal_shader, "u_terminal_size")
                .as_ref(),
            buffer_view.get_buffer().get_width() as f32 - 0.0001,
            buffer_view.calc.forced_height as f32 - 0.0001,
        );

        gl.uniform_1_i32(
            gl.get_uniform_location(self.terminal_shader, "u_fonts")
                .as_ref(),
            FONT_TEXTURE_SLOT as i32,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.terminal_shader, "u_palette")
                .as_ref(),
            PALETTE_TEXTURE_SLOT as i32,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.terminal_shader, "u_terminal_buffer")
                .as_ref(),
            BUFFER_TEXTURE_SLOT as i32,
        );

        gl.uniform_1_i32(
            gl.get_uniform_location(self.terminal_shader, "u_reference_image")
                .as_ref(),
            REFERENCE_IMAGE_TEXTURE_SLOT as i32,
        );

        if let Some(img) = &self.reference_image {
            gl.uniform_2_f32(
                gl.get_uniform_location(self.terminal_shader, "u_reference_image_size")
                    .as_ref(),
                img.width() as f32,
                img.height() as f32,
            );
        }
        gl.uniform_1_f32(
            gl.get_uniform_location(self.terminal_shader, "u_has_reference_image")
                .as_ref(),
            if self.show_reference_image { 1.0 } else { 0.0 },
        );

        gl.uniform_4_f32(
            gl.get_uniform_location(self.terminal_shader, "u_selection_fg")
                .as_ref(),
            monitor_settings.selection_fg.r() as f32 / 255.0,
            monitor_settings.selection_fg.g() as f32 / 255.0,
            monitor_settings.selection_fg.b() as f32 / 255.0,
            monitor_settings.selection_fg.a() as f32 / 255.0,
        );
        gl.uniform_4_f32(
            gl.get_uniform_location(self.terminal_shader, "u_selection_bg")
                .as_ref(),
            monitor_settings.selection_bg.r() as f32 / 255.0,
            monitor_settings.selection_bg.g() as f32 / 255.0,
            monitor_settings.selection_bg.b() as f32 / 255.0,
            monitor_settings.selection_bg.a() as f32 / 255.0,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.terminal_shader, "u_selection_attr")
                .as_ref(),
            if buffer_view.get_buffer().is_terminal_buffer {
                1.0
            } else {
                0.0
            },
        );

        crate::check_gl_error!(gl, "run_shader");
    }
}

unsafe fn compile_shader(gl: &glow::Context) -> glow::Program {
    let program = gl.create_program().expect("Cannot create program");

    let (vertex_shader_source, fragment_shader_source) = (
        prepare_shader!(crate::ui::buffer_view::SHADER_SOURCE),
        prepare_shader!(include_str!("terminal_renderer.shader.frag")),
    );
    let shader_sources = [
        (glow::VERTEX_SHADER, vertex_shader_source),
        (glow::FRAGMENT_SHADER, fragment_shader_source),
    ];

    let shaders: Vec<_> = shader_sources
        .iter()
        .map(|(shader_type, shader_source)| {
            let shader = gl
                .create_shader(*shader_type)
                .expect("Cannot create shader");
            gl.shader_source(
                shader,
                shader_source, /*&format!("{}\n{}", shader_version, shader_source)*/
            );
            gl.compile_shader(shader);
            assert!(
                gl.get_shader_compile_status(shader),
                "{}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            shader
        })
        .collect();

    gl.link_program(program);
    assert!(
        gl.get_program_link_status(program),
        "{}",
        gl.get_program_info_log(program)
    );

    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    crate::check_gl_error!(gl, "compile_shader");

    program
}

unsafe fn create_buffer_texture(gl: &glow::Context) -> glow::Texture {
    let buffer_texture = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(buffer_texture));
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_WRAP_S,
        glow::CLAMP_TO_EDGE as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_WRAP_T,
        glow::CLAMP_TO_EDGE as i32,
    );
    crate::check_gl_error!(gl, "create_buffer_texture");

    buffer_texture
}

unsafe fn create_palette_texture(gl: &glow::Context) -> glow::Texture {
    let palette_texture: glow::Texture = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(palette_texture));

    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_S,
        glow::CLAMP_TO_EDGE as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_T,
        glow::CLAMP_TO_EDGE as i32,
    );
    crate::check_gl_error!(gl, "create_palette_texture");

    palette_texture
}

unsafe fn create_reference_image_texture(gl: &glow::Context) -> glow::Texture {
    let reference_image_texture: glow::Texture = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(reference_image_texture));
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    crate::check_gl_error!(gl, "create_refeference_image_texture");

    reference_image_texture
}

unsafe fn create_font_texture(gl: &glow::Context) -> glow::Texture {
    let font_texture = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(font_texture));

    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_WRAP_S,
        glow::CLAMP_TO_EDGE as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D_ARRAY,
        glow::TEXTURE_WRAP_T,
        glow::CLAMP_TO_EDGE as i32,
    );
    crate::check_gl_error!(gl, "create_font_texture");

    font_texture
}

fn conv_color(c: u32, colors: u32) -> u8 {
    ((255 * c) / colors) as u8
}
