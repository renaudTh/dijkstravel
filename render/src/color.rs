use skia_safe::Color;

/// `#rrggbb` (format garanti par `parseTheme`) ; noir pour tout le reste.
pub fn parse_color(hex: &str) -> Color {
    let value = hex.strip_prefix('#').and_then(|digits| u32::from_str_radix(digits, 16).ok()).unwrap_or(0);
    Color::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hex_colors() {
        assert_eq!(parse_color("#0055a4"), Color::from_rgb(0x00, 0x55, 0xa4));
        assert_eq!(parse_color("pas une couleur"), Color::BLACK);
    }
}
