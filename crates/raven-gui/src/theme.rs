//! What colony-ui needs to know about this program, and what it gives back.
//!
//! Every colour and every font size in the window comes through here. The org
//! rule is that a size is never a raw number: two independent user
//! preferences, Appearance and Accessibility, multiply into one scale, so the
//! layout has to survive 0.7225x to 1.68x, and `sz` is the only thing that
//! knows it.

use colony_ui::{ThemePalette, Typography};
use iced::Font;
use iced::widget::button;

/// The org's application font. JetBrainsMono Nerd Font per
/// Project-Colony-Resources/design/typography.md.
pub const APP_FONT: Font = Font::with_name("JetBrainsMono Nerd Font");

/// The current palette, whichever theme the user picked.
pub fn palette() -> ThemePalette {
    colony_ui::active_palette()
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

/// The type scale. `scale` is 1.0 until the settings screen exists to change
/// it; the accessor is used everywhere from the start so adding that screen
/// later changes one function rather than every call site.
pub fn typography() -> Typography {
    Typography {
        scale: 1.0,
        regular: APP_FONT,
        medium: APP_FONT,
        bold: APP_FONT,
    }
}
