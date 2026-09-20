use eframe::egui::{self, Color32, Visuals};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serialize a Color32 as "#rrggbb" so skins are easy to hand-edit in the
/// config file. Accepts any form `Color32::from_hex` understands.
mod hex_color {
    use super::*;

    pub fn serialize<S: Serializer>(color: &Color32, s: S) -> Result<S::Ok, S::Error> {
        // to_hex always appends alpha; the common opaque case reads cleaner
        // as plain "#rrggbb".
        if color.a() == 255 {
            format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b()).serialize(s)
        } else {
            color.to_hex().serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let s = String::deserialize(d)?;
        Color32::from_hex(&s)
            .map_err(|_| serde::de::Error::custom(format!("invalid hex color: {s:?}")))
    }
}

mod hex_color_opt {
    use super::*;

    pub fn serialize<S: Serializer>(color: &Option<Color32>, s: S) -> Result<S::Ok, S::Error> {
        match color {
            Some(c) => s.serialize_some(&c.to_hex()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color32>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| {
                Color32::from_hex(&s)
                    .map_err(|_| serde::de::Error::custom(format!("invalid hex color: {s:?}")))
            })
            .transpose()
    }
}

/// Palette for the spectrum analyzer: the colors it paints itself rather
/// than pulling from egui visuals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpectrumColors {
    #[serde(with = "hex_color")]
    pub background: Color32,
    /// Unlit segments — the faint grid behind the bars.
    #[serde(with = "hex_color")]
    pub ghost: Color32,
    /// Lit segments below ~62% height.
    #[serde(with = "hex_color")]
    pub low: Color32,
    /// Lit segments between ~62% and ~85% height.
    #[serde(with = "hex_color")]
    pub mid: Color32,
    /// Lit segments above ~85% height.
    #[serde(with = "hex_color")]
    pub high: Color32,
    /// The slowly-falling peak-hold cell.
    #[serde(with = "hex_color")]
    pub peak: Color32,
}

impl Default for SpectrumColors {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(8, 12, 8),
            ghost: Color32::from_rgb(18, 30, 20),
            low: Color32::from_rgb(0, 210, 90),
            mid: Color32::from_rgb(240, 200, 40),
            high: Color32::from_rgb(255, 60, 40),
            peak: Color32::from_rgb(255, 240, 200),
        }
    }
}

/// Everything the skin covers. `None` fields leave the corresponding egui
/// visuals untouched, so a skin can override just the custom-painted parts
/// and inherit the base widget theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
    /// Base widget theme: egui's dark or light visuals.
    pub dark: bool,
    /// Title, marquee, active sort column, current track, play button.
    #[serde(with = "hex_color")]
    pub accent: Color32,
    /// Time labels beside the seek bar.
    #[serde(with = "hex_color")]
    pub text_dim: Color32,
    /// Audio error message.
    #[serde(with = "hex_color")]
    pub error: Color32,
    /// Window/panel background.
    #[serde(with = "hex_color_opt", skip_serializing_if = "Option::is_none")]
    pub background: Option<Color32>,
    /// Selected-row fill in the playlist.
    #[serde(with = "hex_color_opt", skip_serializing_if = "Option::is_none")]
    pub selection: Option<Color32>,
    /// Default text color.
    #[serde(with = "hex_color_opt", skip_serializing_if = "Option::is_none")]
    pub text: Option<Color32>,
    pub spectrum: SpectrumColors,
}

impl Default for Skin {
    fn default() -> Self {
        Self::winamp()
    }
}

impl Skin {
    /// The original look — green-on-black, like classic Winamp.
    pub fn winamp() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(0, 255, 128),
            text_dim: Color32::LIGHT_GRAY,
            error: Color32::RED,
            background: None,
            selection: None,
            text: None,
            spectrum: SpectrumColors::default(),
        }
    }

    /// Amber terminal: warm monochrome with an orange-red top end.
    pub fn amber() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(255, 176, 0),
            text_dim: Color32::from_rgb(200, 170, 120),
            error: Color32::RED,
            background: None,
            selection: None,
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(12, 10, 6),
                ghost: Color32::from_rgb(36, 26, 12),
                low: Color32::from_rgb(220, 150, 0),
                mid: Color32::from_rgb(255, 110, 30),
                high: Color32::from_rgb(255, 60, 40),
                peak: Color32::from_rgb(255, 230, 170),
            },
        }
    }

    /// Cold blue: cyan-on-navy, bars fading to white.
    pub fn ice() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(90, 200, 255),
            text_dim: Color32::LIGHT_GRAY,
            error: Color32::RED,
            background: Some(Color32::from_rgb(10, 16, 24)),
            selection: Some(Color32::from_rgb(20, 60, 100)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(6, 10, 16),
                ghost: Color32::from_rgb(16, 26, 38),
                low: Color32::from_rgb(40, 110, 255),
                mid: Color32::from_rgb(70, 200, 255),
                high: Color32::from_rgb(240, 250, 255),
                peak: Color32::from_rgb(255, 255, 255),
            },
        }
    }

    /// Light mode: egui's light visuals with a green accent.
    pub fn paper() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(0, 120, 70),
            text_dim: Color32::DARK_GRAY,
            error: Color32::RED,
            background: None,
            selection: None,
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(235, 232, 224),
                ghost: Color32::from_rgb(210, 206, 196),
                low: Color32::from_rgb(20, 140, 80),
                mid: Color32::from_rgb(200, 130, 0),
                high: Color32::from_rgb(210, 50, 30),
                peak: Color32::from_rgb(0, 120, 70),
            },
        }
    }

    /// Vaporwave Dream (ff71ce / 01cdfe / 05ffa1 / b967ff / fffb96):
    /// cyan accent on deep purple, bars running violet → pink → cream.
    pub fn vaporwave() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(1, 205, 254),
            text_dim: Color32::from_rgb(190, 170, 210),
            error: Color32::RED,
            background: Some(Color32::from_rgb(16, 11, 26)),
            selection: Some(Color32::from_rgb(80, 40, 110)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(13, 8, 22),
                ghost: Color32::from_rgb(34, 22, 50),
                low: Color32::from_rgb(185, 103, 255),
                mid: Color32::from_rgb(255, 113, 206),
                high: Color32::from_rgb(255, 251, 150),
                peak: Color32::from_rgb(255, 255, 255),
            },
        }
    }

    /// Sunset (264653 / 2a9d8f / e9c46a / f4a261 / e76f51):
    /// orange accent on dark teal, bars running teal → sand → coral.
    pub fn sunset() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(244, 162, 97),
            text_dim: Color32::from_rgb(170, 190, 185),
            error: Color32::RED,
            background: Some(Color32::from_rgb(22, 34, 37)),
            selection: Some(Color32::from_rgb(42, 90, 95)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(16, 26, 27),
                ghost: Color32::from_rgb(30, 48, 48),
                low: Color32::from_rgb(42, 157, 143),
                mid: Color32::from_rgb(233, 196, 106),
                high: Color32::from_rgb(231, 111, 81),
                peak: Color32::from_rgb(244, 162, 97),
            },
        }
    }

    /// Blush (ffcdb2 / ffb4a2 / e5989b / b5838d / 6d6875):
    /// muted rose tones on a dark mauve window.
    pub fn rose() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(255, 180, 162),
            text_dim: Color32::from_rgb(185, 165, 175),
            error: Color32::RED,
            background: Some(Color32::from_rgb(28, 22, 28)),
            selection: Some(Color32::from_rgb(90, 60, 75)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(24, 18, 24),
                ghost: Color32::from_rgb(50, 38, 48),
                low: Color32::from_rgb(181, 131, 141),
                mid: Color32::from_rgb(229, 152, 155),
                high: Color32::from_rgb(255, 205, 178),
                peak: Color32::from_rgb(255, 205, 178),
            },
        }
    }

    /// Candy (cdb4db / ffc8dd / ffafcc / bde0fe / a2d2ff):
    /// pastel pink accent, bars running baby blue → lilac → pink.
    pub fn candy() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(255, 175, 204),
            text_dim: Color32::from_rgb(180, 170, 195),
            error: Color32::RED,
            background: Some(Color32::from_rgb(20, 16, 28)),
            selection: Some(Color32::from_rgb(70, 50, 90)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(16, 12, 24),
                ghost: Color32::from_rgb(38, 30, 50),
                low: Color32::from_rgb(162, 210, 255),
                mid: Color32::from_rgb(205, 180, 219),
                high: Color32::from_rgb(255, 175, 204),
                peak: Color32::from_rgb(255, 255, 255),
            },
        }
    }

    /// Forest (606c38 / 283618 / fefae0 / dda15e / bc6c25):
    /// earthy olive with a tan accent, bars running moss → tan → cream.
    pub fn forest() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(221, 161, 94),
            text_dim: Color32::from_rgb(175, 180, 150),
            error: Color32::RED,
            background: Some(Color32::from_rgb(18, 22, 12)),
            selection: Some(Color32::from_rgb(70, 85, 45)),
            text: None,
            spectrum: SpectrumColors {
                background: Color32::from_rgb(14, 17, 9),
                ghost: Color32::from_rgb(32, 40, 20),
                low: Color32::from_rgb(96, 108, 56),
                mid: Color32::from_rgb(221, 161, 94),
                high: Color32::from_rgb(254, 250, 224),
                peak: Color32::from_rgb(254, 250, 224),
            },
        }
    }

    /// Named built-in skins shown in the picker.
    pub fn presets() -> Vec<(&'static str, Self)> {
        vec![
            ("winamp", Self::winamp()),
            ("amber", Self::amber()),
            ("ice", Self::ice()),
            ("vaporwave", Self::vaporwave()),
            ("sunset", Self::sunset()),
            ("rose", Self::rose()),
            ("candy", Self::candy()),
            ("forest", Self::forest()),
            ("paper", Self::paper()),
        ]
    }

    /// Name of the built-in preset matching this skin, if it matches one
    /// exactly — otherwise the picker shows "custom".
    pub fn preset_name(&self) -> Option<&'static str> {
        Self::presets()
            .into_iter()
            .find(|(_, s)| s == self)
            .map(|(name, _)| name)
    }

    /// Restyle egui's widgets to match the skin. The base visuals come from
    /// `dark`; the `Option` fields override only what's set.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.hyperlink_color = self.accent;
        visuals.selection.stroke.color = self.accent;
        if let Some(bg) = self.background {
            visuals.window_fill = bg;
            visuals.panel_fill = bg;
        }
        if let Some(sel) = self.selection {
            visuals.selection.bg_fill = sel;
        }
        if let Some(text) = self.text {
            visuals.override_text_color = Some(text);
        }
        ctx.set_visuals(visuals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skin_is_winamp() {
        assert_eq!(Skin::default(), Skin::winamp());
    }

    #[test]
    fn skin_roundtrips_through_json() {
        let skin = Skin::ice();
        let json = serde_json::to_string(&skin).unwrap();
        let back: Skin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, skin);
    }

    #[test]
    fn colors_serialize_as_hex() {
        let json = serde_json::to_value(Skin::winamp()).unwrap();
        assert_eq!(json["accent"], "#00ff80");
        // Unset options are omitted entirely.
        assert!(json.get("background").is_none());
    }

    #[test]
    fn missing_fields_fall_back_to_winamp() {
        let skin: Skin = serde_json::from_str(r##"{"accent": "#ff0000"}"##).unwrap();
        assert_eq!(skin.accent, Color32::RED);
        assert!(skin.dark);
        assert_eq!(skin.spectrum, SpectrumColors::default());
    }

    #[test]
    fn invalid_hex_is_rejected() {
        assert!(serde_json::from_str::<Skin>(r##"{"accent": "red"}"##).is_err());
    }

    #[test]
    fn preset_name_matches_builtin_and_not_custom() {
        assert_eq!(Skin::amber().preset_name(), Some("amber"));
        let mut custom = Skin::amber();
        custom.accent = Color32::WHITE;
        assert_eq!(custom.preset_name(), None);
    }

    #[test]
    fn every_preset_names_itself() {
        for (name, skin) in Skin::presets() {
            assert_eq!(skin.preset_name(), Some(name));
        }
    }
}
