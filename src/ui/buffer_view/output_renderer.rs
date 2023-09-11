use egui::PaintCallbackInfo;
use egui::Vec2;
use glow::HasContext as _;
use glow::Texture;
use icy_engine::Buffer;
use icy_engine::TextPane;
use web_time::Instant;

use crate::prepare_shader;
use crate::ui::buffer_view::SHADER_SOURCE;
use crate::BufferView;
use crate::TerminalCalc;
use crate::TerminalOptions;

pub const MONO_COLORS: [(u8, u8, u8); 5] = [
    (0xFF, 0xFF, 0xFF), // Black / White
    (0xFF, 0x81, 0x00), // Amber
    (0x0C, 0xCC, 0x68), // Green
    (0x00, 0xD5, 0x6D), // Apple ][
    (0x72, 0x9F, 0xCF), // Futuristic
];

pub struct OutputRenderer {
    output_shader: glow::Program,

    pub framebuffer: glow::Framebuffer,
    pub render_texture: glow::Texture,
    pub render_buffer_size: Vec2,
    pub vertex_array: glow::VertexArray,
    instant: Instant,
}

impl OutputRenderer {
    pub fn new(gl: &glow::Context, buf: &Buffer, calc: &TerminalCalc, filter: i32) -> Self {
        unsafe {
            let w = buf.get_font_dimensions().width as f32
                + if buf.use_letter_spacing() { 1.0 } else { 0.0 };

            let render_buffer_size = Vec2::new(
                w * buf.get_width() as f32,
                buf.get_font_dimensions().height as f32 * calc.forced_height as f32,
            );

            let output_shader = compile_output_shader(gl);
            let framebuffer = gl.create_framebuffer().unwrap();
            let render_texture = create_screen_render_texture(gl, render_buffer_size, filter);
            let vertex_array = gl
                .create_vertex_array()
                .expect("Cannot create vertex array");
            Self {
                output_shader,
                framebuffer,
                render_texture,
                render_buffer_size,
                vertex_array,
                instant: Instant::now(),
            }
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.output_shader);
            gl.delete_vertex_array(self.vertex_array);
            gl.delete_texture(self.render_texture);
            gl.delete_framebuffer(self.framebuffer);
        }
    }

    pub(crate) unsafe fn init_output(&self, gl: &glow::Context) {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
        gl.bind_texture(glow::TEXTURE_2D, Some(self.render_texture));
        gl.viewport(
            0,
            0,
            self.render_buffer_size.x as i32,
            self.render_buffer_size.y as i32,
        );
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(self.render_texture),
            0,
        );

        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.clear_color(0., 0., 0., 0.0);
        crate::check_gl_error!(gl, "init_output");
    }

    pub unsafe fn render_to_screen(
        &self,
        gl: &glow::Context,
        info: &PaintCallbackInfo,
        buffer_view: &BufferView,
        output_texture: glow::Texture,
        calc: &TerminalCalc,
        options: &TerminalOptions,
    ) {
        let monitor_settings = &options.settings;
        let buffer_rect = calc.buffer_rect;
        let terminal_rect = calc.terminal_rect;
        let top_pos = buffer_view.viewport_top.floor();

        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.viewport(
            (terminal_rect.left() * info.pixels_per_point) as i32,
            (info.screen_size_px[1] as f32 - terminal_rect.max.y * info.pixels_per_point) as i32,
            (terminal_rect.width() * info.pixels_per_point) as i32,
            (terminal_rect.height() * info.pixels_per_point) as i32,
        );

        gl.scissor(
            (terminal_rect.left() * info.pixels_per_point) as i32,
            (info.screen_size_px[1] as f32 - terminal_rect.max.y * info.pixels_per_point) as i32,
            (terminal_rect.width() * info.pixels_per_point) as i32,
            (terminal_rect.height() * info.pixels_per_point) as i32,
        );

        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.use_program(Some(self.output_shader));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(output_texture));

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "u_time")
                .as_ref(),
            self.instant.elapsed().as_millis() as f32 / 300.0,
        );

        gl.uniform_1_i32(
            gl.get_uniform_location(self.output_shader, "u_render_texture")
                .as_ref(),
            0,
        );
        let eff = match monitor_settings.background_effect {
            crate::BackgroundEffect::None => {
                if monitor_settings.use_filter {
                    1.0
                } else {
                    0.0
                }
            }
            crate::BackgroundEffect::Checkers => 2.0,
        };
        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "u_effect")
                .as_ref(),
            eff,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "u_use_monochrome")
                .as_ref(),
            if monitor_settings.monitor_type > 0 {
                1.0
            } else {
                0.0
            },
        );

        if monitor_settings.monitor_type > 0 {
            let r = MONO_COLORS[monitor_settings.monitor_type - 1].0 as f32 / 255.0;
            let g = MONO_COLORS[monitor_settings.monitor_type - 1].1 as f32 / 255.0;
            let b = MONO_COLORS[monitor_settings.monitor_type - 1].2 as f32 / 255.0;
            gl.uniform_3_f32(
                gl.get_uniform_location(self.output_shader, "u_monchrome_mask")
                    .as_ref(),
                r,
                g,
                b,
            );
        }

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "gamma")
                .as_ref(),
            monitor_settings.gamma / 50.0,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "contrast")
                .as_ref(),
            monitor_settings.contrast / 50.0,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "saturation")
                .as_ref(),
            monitor_settings.saturation / 50.0,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "brightness")
                .as_ref(),
            monitor_settings.brightness / 30.0,
        );
        /*
                    gl.uniform_1_f32(
                        gl.get_uniform_location(self.draw_program, "light")
                            .as_ref(),
                            self.light);
        */
        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "blur").as_ref(),
            monitor_settings.blur / 30.0,
        );

        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "curvature")
                .as_ref(),
            monitor_settings.curvature / 30.0,
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.output_shader, "u_scanlines")
                .as_ref(),
            0.5 * (monitor_settings.scanlines / 100.0),
        );

        gl.uniform_2_f32(
            gl.get_uniform_location(self.output_shader, "u_resolution")
                .as_ref(),
            terminal_rect.width() * info.pixels_per_point,
            terminal_rect.height() * info.pixels_per_point,
        );

        gl.uniform_4_f32(
            gl.get_uniform_location(self.output_shader, "u_buffer_rect")
                .as_ref(),
            buffer_rect.left() / terminal_rect.width(),
            (info.screen_size_px[1] as f32 - buffer_rect.max.y * info.pixels_per_point)
                / (terminal_rect.height() * info.pixels_per_point),
            buffer_rect.right() / terminal_rect.width(),
            (info.screen_size_px[1] as f32 - buffer_rect.min.y * info.pixels_per_point)
                / (terminal_rect.height() * info.pixels_per_point),
        );

        let y = calc.buffer_rect.top() - calc.char_scroll_positon;
        let y = info.screen_size_px[1] as f32 - y * info.pixels_per_point;
        gl.uniform_2_f32(
            gl.get_uniform_location(self.output_shader, "u_scroll_position")
                .as_ref(),
            (calc.buffer_rect.left() * info.pixels_per_point).floor() + 0.5,
            (y).floor() + 0.5,
        );

        if let Some(raster) = &options.raster {
            gl.uniform_2_f32(
                gl.get_uniform_location(self.output_shader, "u_raster")
                    .as_ref(), // HACK! some raster positions need correction no idea why
                (raster.x * calc.char_size.x).floor() * info.pixels_per_point
                    + if (raster.x as i32) % 3 == 0 {
                        -0.5
                    } else {
                        0.5
                    },
                (raster.y * calc.char_size.y).floor() * info.pixels_per_point,
            );
        } else {
            gl.uniform_2_f32(
                gl.get_uniform_location(self.output_shader, "u_raster")
                    .as_ref(),
                0.0,
                0.0,
            );
        }

        if let Some(guide) = &options.guide {
            gl.uniform_2_f32(
                gl.get_uniform_location(self.output_shader, "u_guide")
                    .as_ref(),
                (guide.x * calc.char_size.x).floor() * info.pixels_per_point,
                (-guide.y * calc.char_size.y).floor(),
            );
        } else {
            gl.uniform_2_f32(
                gl.get_uniform_location(self.output_shader, "u_guide")
                    .as_ref(),
                0.0,
                0.0,
            );
        }

        if let Some(layer) = buffer_view.edit_state.get_cur_layer() {
            if !buffer_view.get_buffer().is_terminal_buffer {
                if let Some(po) = layer.get_preview_offset() {
                    let layer_x = po.x as f32 * calc.char_size.x;
                    let layer_y = po.y as f32 * calc.char_size.y - top_pos;
                    let layer_w = layer.get_width() as f32 * calc.char_size.x;
                    let layer_h = layer.get_height() as f32 * calc.char_size.y;
                    let x = buffer_rect.left() + layer_x;
                    let y = buffer_rect.top() + layer_y;
                    let y = info.screen_size_px[1] as f32 - y * info.pixels_per_point;
                    gl.uniform_4_f32(
                        gl.get_uniform_location(self.output_shader, "u_preview_layer_rectangle")
                            .as_ref(),
                        x * info.pixels_per_point,
                        y - layer_h * info.pixels_per_point,
                        (x + layer_w) * info.pixels_per_point,
                        y,
                    );

                    gl.uniform_3_f32(
                        gl.get_uniform_location(
                            self.output_shader,
                            "u_preview_layer_rectangle_color",
                        )
                        .as_ref(),
                        1.0,
                        1.0,
                        1.0,
                    );
                } else {
                    gl.uniform_4_f32(
                        gl.get_uniform_location(self.output_shader, "u_preview_layer_rectangle")
                            .as_ref(),
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    );

                    gl.uniform_3_f32(
                        gl.get_uniform_location(
                            self.output_shader,
                            "u_preview_layer_rectangle_color",
                        )
                        .as_ref(),
                        1.0,
                        1.0,
                        1.0,
                    );
                }

                let layer_x = layer.get_base_offset().x as f32 * calc.char_size.x;
                let layer_y = layer.get_base_offset().y as f32 * calc.char_size.y - top_pos;
                let layer_w = layer.get_width() as f32 * calc.char_size.x;
                let layer_h = layer.get_height() as f32 * calc.char_size.y;
                let x = buffer_rect.left() + layer_x;
                let y = buffer_rect.top() + layer_y;
                let y = info.screen_size_px[1] as f32 - y * info.pixels_per_point;
                gl.uniform_4_f32(
                    gl.get_uniform_location(self.output_shader, "u_layer_rectangle")
                        .as_ref(),
                    x * info.pixels_per_point,
                    y - layer_h * info.pixels_per_point,
                    (x + layer_w) * info.pixels_per_point,
                    y,
                );
                match layer.role {
                    icy_engine::Role::Normal | icy_engine::Role::Image => {
                        gl.uniform_3_f32(
                            gl.get_uniform_location(self.output_shader, "u_layer_rectangle_color")
                                .as_ref(),
                            1.0,
                            1.0,
                            0.0,
                        );
                    }
                    icy_engine::Role::PastePreview | icy_engine::Role::PasteImage => {
                        gl.uniform_3_f32(
                            gl.get_uniform_location(self.output_shader, "u_layer_rectangle_color")
                                .as_ref(),
                            240. / 255.,
                            230. / 255.,
                            40. / 255.,
                        );
                    }
                }
            } else {
                gl.uniform_3_f32(
                    gl.get_uniform_location(self.output_shader, "u_layer_rectangle_color")
                        .as_ref(),
                    0.0,
                    0.0,
                    0.0,
                );
            }
        } else {
            gl.uniform_3_f32(
                gl.get_uniform_location(self.output_shader, "u_layer_rectangle_color")
                    .as_ref(),
                0.0,
                0.0,
                0.0,
            );
        }

        match buffer_view.get_selection() {
            Some(selection) => {
                if selection.is_empty() || buffer_view.get_buffer().is_terminal_buffer {
                    gl.uniform_4_f32(
                        gl.get_uniform_location(self.output_shader, "u_selection_rectangle")
                            .as_ref(),
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    );
                } else {
                    let border = 0.0;
                    let layer = selection.as_rectangle();
                    let layer_x = layer.left() as f32 * calc.char_size.x;
                    let layer_y = layer.top() as f32 * calc.char_size.y - border - top_pos;
                    let layer_w = layer.get_width() as f32 * calc.char_size.x + border * 2.0;
                    let layer_h = layer.get_height() as f32 * calc.char_size.y + border * 2.0;
                    let x = buffer_rect.left() + layer_x - border;
                    let y = buffer_rect.top() + layer_y;
                    let y = info.screen_size_px[1] as f32 - y * info.pixels_per_point - border;
                    gl.uniform_4_f32(
                        gl.get_uniform_location(self.output_shader, "u_selection_rectangle")
                            .as_ref(),
                        x * info.pixels_per_point,
                        y - layer_h * info.pixels_per_point,
                        (x + layer_w) * info.pixels_per_point,
                        y,
                    );
                }
            }
            None => {
                gl.uniform_4_f32(
                    gl.get_uniform_location(self.output_shader, "u_selection_rectangle")
                        .as_ref(),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                );
            }
        }

        gl.bind_vertex_array(Some(self.vertex_array));
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
    }

    pub(crate) fn update_render_buffer(
        &mut self,
        gl: &glow::Context,
        buf: &Buffer,
        calc: &TerminalCalc,
        scale_filter: i32,
    ) {
        let w = buf.get_font_dimensions().width as f32
            + if buf.use_letter_spacing() { 1.0 } else { 0.0 };

        let render_buffer_size = Vec2::new(
            w * buf.get_width() as f32,
            buf.get_font_dimensions().height as f32 * calc.forced_height as f32,
        );
        if render_buffer_size == self.render_buffer_size {
            return;
        }
        unsafe {
            use glow::HasContext as _;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
            gl.delete_texture(self.render_texture);

            let render_texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(render_texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                render_buffer_size.x as i32,
                render_buffer_size.y as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, scale_filter);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, scale_filter);
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

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.render_texture = render_texture;
            self.render_buffer_size = render_buffer_size;
        }
    }
}

unsafe fn compile_output_shader(gl: &glow::Context) -> glow::Program {
    let draw_program = gl.create_program().expect("Cannot create program");
    let (vertex_shader_source, fragment_shader_source) = (
        prepare_shader!(SHADER_SOURCE),
        prepare_shader!(include_str!("output_renderer.shader.frag")),
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
            gl.attach_shader(draw_program, shader);
            shader
        })
        .collect();

    gl.link_program(draw_program);
    assert!(
        gl.get_program_link_status(draw_program),
        "{}",
        gl.get_program_info_log(draw_program)
    );

    for shader in shaders {
        gl.detach_shader(draw_program, shader);
        gl.delete_shader(shader);
    }
    draw_program
}

unsafe fn create_screen_render_texture(
    gl: &glow::Context,
    render_buffer_size: Vec2,
    filter: i32,
) -> Texture {
    let render_texture = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(render_texture));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA as i32,
        render_buffer_size.x as i32,
        render_buffer_size.y as i32,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        None,
    );
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter);
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

    render_texture
}
