pub mod ui;
use egui::Color32;
use icy_engine::Color;
pub use ui::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorSettings {
    pub use_filter: bool,

    pub monitor_type: usize,

    pub gamma: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub brightness: f32,
    pub light: f32,
    pub blur: f32,
    pub curvature: f32,
    pub scanlines: f32,

    pub background_effect: BackgroundEffect,
    pub selection_fg: Color32,
    pub selection_bg: Color32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundEffect {
    None,
    Checkers,
}

unsafe impl Send for MonitorSettings {}
unsafe impl Sync for MonitorSettings {}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            use_filter: false,
            monitor_type: 0,
            gamma: 50.,
            contrast: 50.,
            saturation: 50.,
            brightness: 30.,
            light: 40.,
            blur: 30.,
            curvature: 10.,
            scanlines: 10.,
            background_effect: BackgroundEffect::None,
            selection_fg: Color32::from_rgb(0xAB, 0x00, 0xAB),
            selection_bg: Color32::from_rgb(0xAB, 0xAB, 0xAB),
        }
    }
}
