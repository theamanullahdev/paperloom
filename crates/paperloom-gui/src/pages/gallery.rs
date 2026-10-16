//! Sizing shared by the card grids (local library and online browser) so
//! both read as the same gallery.

/// Approximation of the column width `Grid::fluid` settles on.
pub const FLUID_COLUMN_WIDTH: f32 = 260.0;
const CARD_CHROME_HEIGHT: f32 = 76.0;

/// Single `Grid` cell ratio such that the preview area matches the real
/// screen's aspect ratio (wallpapers are cropped to fill — `panscan=1.0`
/// in the engine — so the gallery should show that crop) and the card's
/// fixed-height chrome (title + button row) sits below it.
///
/// `Grid` only sizes whole cells by one ratio, so the chrome height is
/// folded in: `image_height = col_width / screen_ratio`, then
/// `card_ratio = col_width / (image_height + chrome)`.
pub fn card_ratio() -> f32 {
    let (screen_w, screen_h) = paperloom_common::primary_aspect_ratio();
    let image_height = FLUID_COLUMN_WIDTH * screen_h as f32 / screen_w as f32;
    FLUID_COLUMN_WIDTH / (image_height + CARD_CHROME_HEIGHT)
}
