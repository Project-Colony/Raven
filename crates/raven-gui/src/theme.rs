//! What colony-ui needs to know about this program, and what it gives back.
//!
//! Every colour and every font size in the window comes through here. The org
//! rule is that a size is never a raw number: two independent user
//! preferences, Appearance and Accessibility, multiply into one scale, so the
//! layout has to survive 0.7225x to 1.68x, and `sz` is the only thing that
//! knows it.

use colony_ui::{ThemePalette, Typography};
use iced::Font;

/// The org's application font. JetBrainsMono Nerd Font per
/// Project-Colony-Resources/design/typography.md.
pub const APP_FONT: Font = Font::with_name("JetBrainsMono Nerd Font");

/// The current palette, whichever theme the user picked.
pub fn palette() -> ThemePalette {
    colony_ui::active_palette()
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
