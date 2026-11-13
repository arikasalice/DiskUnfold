use super::*;

/// Value of a custom property in the light block (first definition) or the dark block (inside
/// the `prefers-color-scheme: dark` rule) of the GUI tokens.
fn token(css: &str, block: &str, name: &str) -> Option<u32> {
    let rest = &css[css.find(block)?..];
    let at = rest.find(&format!("{name}: #"))? + name.len() + 3;
    u32::from_str_radix(rest.get(at..at + 6)?, 16).ok()
}
#[test]
fn the_palettes_equal_the_gui_tokens() {
    let css = include_str!("../../../../../gui-web/src/theme/tokens.css");
    for (palette, block) in [
        (LIGHT, ":root {"),
        (DARK, "@media (prefers-color-scheme: dark)"),
    ] {
        assert_eq!(Some(palette.background), token(css, block, "--du-page"));
        assert_eq!(Some(palette.text), token(css, block, "--du-text"));
        assert_eq!(Some(palette.border), token(css, block, "--du-border"));
    }
}

#[test]
fn colorref_swaps_red_and_blue() {
    assert_eq!(colorref(0xf5f5f6), 0xf6f5f5);
    assert_eq!(colorref(0x112233), 0x332211);
}

#[test]
fn the_caption_title_is_one_bounded_line() {
    assert_eq!(
        caption_title("DiskUnfold \u{2014} Startup"),
        "DiskUnfold \u{2014} Startup"
    );
    assert_eq!(caption_title("a\nb\tc"), "a b c");
    assert_eq!(caption_title("  \n "), "");
    let long = "x".repeat(MAX_TITLE + 40);
    assert_eq!(caption_title(&long).len(), MAX_TITLE);
    // A pair of UTF-16 units is never split at the limit.
    let emoji = "\u{1F4BF}".repeat(MAX_TITLE);
    assert_eq!(caption_title(&emoji).encode_utf16().count(), MAX_TITLE);
}
