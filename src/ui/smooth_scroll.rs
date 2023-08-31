use egui::{Color32, Id, Pos2, Rect, Response, Sense, Ui, Vec2};

use crate::TerminalCalc;

pub struct SmoothScroll {
    /// Current scroll position in terminal pixels (not screen pixels)
    char_scroll_positon: f32,
    /// used to determine if the buffer should auto scroll to the bottom.
    last_char_height: f32,
    drag_start: bool,
    id: Id,
    lock_focus: bool,
    stick_to_bottom: bool,
    scroll_offset: Option<f32>,
    /// Scroll position set by the user
    set_scroll_positon: bool,
}

impl Default for SmoothScroll {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothScroll {
    pub fn new() -> Self {
        Self {
            id: Id::new("smooth_scroll"),
            char_scroll_positon: 0.0,
            last_char_height: 0.0,
            drag_start: false,
            lock_focus: true,
            stick_to_bottom: true,
            scroll_offset: None,
            set_scroll_positon: false,
        }
    }

    pub fn with_id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }

    pub fn with_lock_focus(mut self, lock_focus: bool) -> Self {
        self.lock_focus = lock_focus;
        self
    }

    pub(crate) fn with_stick_to_bottom(mut self, stick_to_bottom: bool) -> Self {
        self.stick_to_bottom = stick_to_bottom;
        self
    }

    pub(crate) fn with_scroll_offset(mut self, scroll_offset: Option<f32>) -> Self {
        self.scroll_offset = scroll_offset;
        self
    }

    fn persist_data(&mut self, ui: &Ui) {
        ui.ctx().memory_mut(|mem: &mut egui::Memory| {
            mem.data.insert_persisted(
                self.id,
                (
                    self.char_scroll_positon,
                    self.last_char_height,
                    self.drag_start,
                ),
            );
        });
    }

    fn load_data(&mut self, ui: &Ui) {
        if let Some(scroll) = ui
            .ctx()
            .memory_mut(|mem| mem.data.get_persisted::<(f32, f32, bool)>(self.id))
        {
            self.char_scroll_positon = scroll.0;
            self.last_char_height = scroll.1;
            self.drag_start = scroll.2;
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        calc_contents: impl FnOnce(Rect) -> TerminalCalc,
        add_contents: impl FnOnce(&mut Ui, &mut TerminalCalc),
    ) -> (Response, TerminalCalc) {
        self.load_data(ui);
        let size = ui.available_size();

        let (_, rect) = ui.allocate_space(Vec2::new(size.x, size.y));
        let mut response = ui.interact(rect, self.id, Sense::click_and_drag());

        if self.lock_focus {
            self.lock_focus = false;
            ui.memory_mut(|m| {
                m.request_focus(self.id);
                m.lock_focus(self.id, true);
            });
        }

        let mut calc = calc_contents(rect);
        calc.char_scroll_positon = self.char_scroll_positon;

        if self.stick_to_bottom && (calc.char_height - self.last_char_height).abs() > 0.1 {
            self.char_scroll_positon =
                calc.font_height * (calc.char_height - calc.buffer_char_height).max(0.0);
        }
        self.last_char_height = calc.char_height;

        if let Some(sp) = self.scroll_offset {
            self.char_scroll_positon = sp;
        }
        self.clamp_scroll_position(&mut calc);

        let scrollbar_width = ui.style().spacing.scroll_bar_width;
        let x = rect.right() - scrollbar_width;
        let mut scrollbar_rect: Rect = rect;
        scrollbar_rect.set_left(x);
        calc.scrollbar_rect = scrollbar_rect;

        add_contents(ui, &mut calc);

        if calc.char_height > calc.buffer_char_height {
            self.clamp_scroll_position(&mut calc);
            response = self.show_scrollbar(ui, response, &mut calc);
        }

        self.persist_data(ui);
        calc.set_scroll_position_set_by_user = self.set_scroll_positon;

        self.clamp_scroll_position(&mut calc);

        (response, calc)
    }

    fn clamp_scroll_position(&mut self, calc: &mut TerminalCalc) {
        let mut max: f32 = calc.font_height * (calc.char_height - calc.buffer_char_height).max(0.0);

        // HACK around cutting the last line - I'm sure the error is somewhere else, but this works.
        max += calc.scroll_remainder * (calc.font_height - 1.0);

        self.char_scroll_positon = self.char_scroll_positon.clamp(0.0, max);
        calc.char_scroll_positon = self.char_scroll_positon;
    }

    fn show_scrollbar(&mut self, ui: &Ui, response: Response, calc: &mut TerminalCalc) -> Response {
        let scrollbar_width = ui.style().spacing.scroll_bar_width;
        let x = calc.terminal_rect.right() - scrollbar_width;
        let mut bg_rect: Rect = calc.terminal_rect;
        bg_rect.set_left(x);

        // HACK for scroll remainder workaround:
        let real_char_height = calc.scroll_remainder + calc.char_height.max(1.0);
        let bar_height = (calc.buffer_char_height / real_char_height) * calc.terminal_rect.height();
        let bar_offset = -bar_height / 2.0;

        let how_on = if ui.is_enabled() {
            let (dragged, hovered) =
                self.handle_user_input(ui, &response, x, bar_offset, calc, bg_rect);
            self.clamp_scroll_position(calc);
            ui.ctx().animate_bool(response.id, hovered || dragged)
        } else {
            0.0
        };

        let x_size = egui::lerp(2.0..=scrollbar_width, how_on);

        // draw bg
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(calc.terminal_rect.right() - x_size, bg_rect.top()),
                Vec2::new(x_size, calc.terminal_rect.height()),
            ),
            0.,
            Color32::from_rgba_unmultiplied(0x3F, 0x3F, 0x3F, 32),
        );

        // draw bar
        let bar_top = calc.terminal_rect.top()
            + calc.terminal_rect.height() * self.char_scroll_positon
                / (calc.font_height * real_char_height);
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(calc.terminal_rect.right() - x_size, bar_top),
                Vec2::new(x_size, bar_height),
            ),
            4.,
            Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0x5F + (127.0 * how_on) as u8),
        );
        response
    }

    fn handle_user_input(
        &mut self,
        ui: &Ui,
        response: &Response,
        x: f32,
        bar_offset: f32,
        calc: &TerminalCalc,
        bg_rect: Rect,
    ) -> (bool, bool) {
        if response.clicked() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                if mouse_pos.x > x {
                    let my = mouse_pos.y + bar_offset;
                    self.char_scroll_positon =
                        calc.char_height * calc.font_height * (my - bg_rect.top())
                            / bg_rect.height();
                    self.set_scroll_positon = true;
                }
            }
        }

        let mut dragged: bool = false;

        if self.drag_start && response.dragged() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                dragged = true;
                let my = mouse_pos.y + bar_offset;
                self.char_scroll_positon =
                    calc.char_height * calc.font_height * (my - bg_rect.top()) / bg_rect.height();
                self.set_scroll_positon = true;
            }
        }
        let mut hovered = false;
        if response.hovered() {
            let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
            for e in events {
                if let egui::Event::Scroll(vec) = e {
                    self.char_scroll_positon -= vec.y;
                    self.set_scroll_positon = true;
                }
            }

            if let Some(mouse_pos) = response.hover_pos() {
                if mouse_pos.x > x {
                    hovered = true;
                }
            }
        }

        if hovered && response.drag_started() {
            self.drag_start = true;
        }

        if response.drag_released() {
            self.drag_start = false;
        }
        (dragged, hovered)
    }
}
