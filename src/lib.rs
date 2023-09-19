pub mod ui;
use egui::Color32;
use serde::{Serialize, Deserialize};
pub use ui::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerSettings {
    pub border_color: Color32,

    pub reference_image_alpha: f32,

    pub raster_alpha: f32,
    pub raster_color: Color32,

    pub guide_alpha: f32,
    pub guide_color: Color32,
} 

impl Default for MarkerSettings {
    fn default() -> Self {
        Self {
            reference_image_alpha: 0.2,
            raster_alpha: 0.2,
            raster_color: Color32::from_rgb(0xAB, 0xAB, 0xAB),
            guide_alpha: 0.2,
            guide_color: Color32::from_rgb(0xAB, 0xAB, 0xAB),

            border_color: Color32::from_rgb(64, 69, 74),
        }
    }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

use rust_embed::RustEmbed;
use i18n_embed::{
    fluent::{fluent_language_loader, FluentLanguageLoader},
    DesktopLanguageRequester,
};

#[derive(RustEmbed)]
#[folder = "i18n"] // path to the compiled localization resources
struct Localizations;

use once_cell::sync::Lazy;
static LANGUAGE_LOADER: Lazy<FluentLanguageLoader> = Lazy::new(|| {
    let loader = fluent_language_loader!();
    let requested_languages = DesktopLanguageRequester::requested_languages();
    let _result = i18n_embed::select(&loader, &Localizations, &requested_languages);
    loader
});