//! What colony-ui needs to know about this program, and what it gives back.
//!
//! Every colour in the window comes through here. Sizes and fonts come from
//! `colony_ui::Typography::current()`, read once per view: the two text-size
//! preferences, Appearance and Accessibility, multiply into its scale, so a
//! size is never a raw number and the layout has to survive 0.7225x to 1.68x;
//! its fonts are OpenDyslexic while the dyslexia font is on.

use colony_ui::ThemePalette;
use iced::widget::button;

/// The active colony-ui palette - the user's theme, with high contrast when
/// it is on - and the user's accent in `accent_blue`, which colony-ui keeps
/// apart from the palette so the theme's own can come back.
pub fn palette() -> ThemePalette {
    ThemePalette {
        accent_blue: colony_ui::effective_accent(),
        ..colony_ui::active_palette()
    }
}

/// The look of an ordinary button, for every ordinary button in the window.
///
/// A button left without `.style()` does not fall back to nothing - it falls
/// back to iced's own theme, which paints its blue on colony-ui's ground. One
/// helper rather than a copy per call site, so the three button colours the
/// palette carries are used everywhere or nowhere.
///
/// `Disabled` keeps the resting fill and dims the label: the palette has no
/// disabled token, and a button that vanishes when it cannot be pressed is
/// harder to find again than one that greys out.
pub fn button_style(p: ThemePalette) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(
            match status {
                button::Status::Hovered => p.btn_hover,
                button::Status::Pressed => p.btn_pressed,
                button::Status::Active | button::Status::Disabled => p.btn_default,
            }
            .into(),
        ),
        text_color: match status {
            button::Status::Disabled => p.text_muted,
            _ => p.text_primary,
        },
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
