













pub fn rgb_to_cmyk(rgb: (u8, u8, u8)) -> (u8, u8, u8, u8) {
    

    
    let (r, g, b) = (
        rgb.0 as f64 / 255f64,
        rgb.1 as f64 / 255f64,
        rgb.2 as f64 / 255f64,
    );

    match 1f64 - r.max(g).max(b) {
        1f64 => (0, 0, 0, 100), 
        k => (
            (100f64 * (1f64 - r - k) / (1f64 - k)) as u8, 
            (100f64 * (1f64 - g - k) / (1f64 - k)) as u8, 
            (100f64 * (1f64 - b - k) / (1f64 - k)) as u8, 
            (100f64 * k) as u8,                           
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_rgb_to_cmyk {
        ($($name:ident: $tc:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (rgb, cmyk) = $tc;
                    assert_eq!(rgb_to_cmyk(rgb), cmyk);
                }
            )*
        }
    }

    test_rgb_to_cmyk! {
        white: ((255, 255, 255), (0, 0, 0, 0)),
        gray: ((128, 128, 128), (0, 0, 0, 49)),
        black: ((0, 0, 0), (0, 0, 0, 100)),
        red: ((255, 0, 0), (0, 100, 100, 0)),
        green: ((0, 255, 0), (100, 0, 100, 0)),
        blue: ((0, 0, 255), (100, 100, 0, 0)),
    }
}
