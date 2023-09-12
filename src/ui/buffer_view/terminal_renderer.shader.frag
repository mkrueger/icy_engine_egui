precision highp float;
precision lowp sampler2DArray;

uniform sampler2DArray u_fonts;
uniform sampler2D u_palette;
uniform sampler2DArray u_terminal_buffer;

uniform vec2        u_resolution;
uniform vec2        u_output_resolution;

uniform vec2        u_position;
uniform vec2        u_terminal_size;
uniform vec4        u_caret_rectangle;

uniform float       u_selection_attr;
uniform vec4        u_selection_fg;
uniform vec4        u_selection_bg;

uniform float       u_character_blink;

uniform sampler2D   u_reference_image;
uniform float       u_has_reference_image;
uniform vec2        u_reference_image_size;

out     vec4        color1;
out     vec4        color2;

vec4 get_char(vec2 p, float c, float page) {
    if (p.x < 0.|| p.x > 1. || p.y < 0.|| p.y > 1.) {
        return vec4(0, 0, 0, 1.0);
    }
    vec2 v = p / 16.0 + fract(vec2(c, floor(c / 16.0)) / 16.0);
    return textureGrad(u_fonts, vec3(v, page), dFdx(p / 16.0), dFdy(p / 16.0));
}

vec4 get_palette_color(float c) {
    return texture(u_palette, vec2(c, 0));
}

bool check_bit(float v, int bit) {
    return (int(255.0 * v) & (1 << bit)) != 0;
}

void main (void) {
    vec2 view_coord = (gl_FragCoord.xy - u_position) / u_resolution;
    view_coord = vec2(view_coord.s, 1.0 - view_coord.t);

    vec2 fb_pos = view_coord * u_terminal_size;

    // get char and attributs from the terminal background buffer
    vec2 terminal_buffer_coordinates = (gl_FragCoord.xy - u_position) / u_output_resolution;
    terminal_buffer_coordinates = vec2(terminal_buffer_coordinates.s, 1.0 - terminal_buffer_coordinates.t);
    vec4 ch = texture(u_terminal_buffer, vec3(terminal_buffer_coordinates, 0.0));
    vec4 ch_attr = texture(u_terminal_buffer, vec3(terminal_buffer_coordinates, 1.0));
    
    vec2 fract_fb_pos = fract(vec2(fb_pos.x, fb_pos.y));

    float ch_value = ch.x * 255.0;
    // double height
    if (check_bit(ch_attr[0], 3)) {
        fract_fb_pos.y /= 2.0;
        // 2nd line
        if (check_bit(ch_attr[0], 4)) {
            fract_fb_pos.y += 0.5;
        }
    }

    vec4 char_data = get_char(fract_fb_pos, ch_value, ch.a * 255.0);
    
    vec4 fg = get_palette_color(ch.y);
    vec4 bg = get_palette_color(ch.z);

   // if (u_selection_attr > 0.0) {
        float x = floor(fb_pos.x);
        float y = floor(fb_pos.y);
        if (ch_attr.b > 0.0) {
            color2 = vec4(1.0, 0.0, 0.0, 1.0);
            if (u_selection_bg.w > 0.0) {
                bg = u_selection_bg;
            } else {
                bg = fg;
            }
            if (u_selection_fg.w > 0.0) {
                fg = u_selection_fg;
            }
        } else {
            color2 = vec4(0.0, 0.0, 1.0, 1.0);
        }
  //  }

    if (abs(ch_attr[3] - 0.5) < 0.1) {
        color1 = vec4(0.0);
    } else {
        if (char_data.x > 0.5 && (ch_attr[3] == 0.0 || u_character_blink > 0.0)) {
            color1 = fg;
        } else {
            color1 = bg;
        }
        // underline
        if (check_bit(ch_attr[0], 0)) {
            if (fract_fb_pos.y >= 15.0 / 16.0) {
                color1 = fg;
            }
        }

        // double underline
        if (check_bit(ch_attr[0], 1)) {
            if (fract_fb_pos.y >= 13.0 / 16.0 && fract_fb_pos.y < 14.0 / 16.0) {
                color1 = fg;
            }
        }

        // strike through
        if (check_bit(ch_attr[0], 2)) {
            if (fract_fb_pos.y >= 7.0 / 16.0 && fract_fb_pos.y < 8.0 / 16.0) {
                color1 = fg;
            }
        }
    }

    if (u_has_reference_image > 0.5) {
        vec2 view_coord = gl_FragCoord.xy / u_reference_image_size;
        view_coord = vec2(view_coord.s, 1.0 - view_coord.t);

        vec4 img = texture(u_reference_image, view_coord);
        color1 = 0.2 * img + color1 * 0.8;
    }

    // paint caret

    vec2 upper_left = u_caret_rectangle.xy;
    vec2 bottom_right = u_caret_rectangle.zw;

    if (upper_left.x <= terminal_buffer_coordinates.x && 
        upper_left.y <= terminal_buffer_coordinates.y && 
        terminal_buffer_coordinates.x < bottom_right.x && 
        terminal_buffer_coordinates.y < bottom_right.y) {
        color1 = get_palette_color(ch.y);
    } 
}