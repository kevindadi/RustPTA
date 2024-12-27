pub mod lola;
pub mod tina;
pub mod tsan;

pub fn normalize_name(name: &str) -> String {
    name.replace("::", ".")
        .replace("{", "")
        .replace("}", "")
        .replace("#", "")
}

pub fn normalize_name_for_tina(name: &str) -> String {
    name.replace("::", "")
        .replace("{", "")
        .replace("}", "")
        .replace("#", "")
        .replace("_", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_normalization() {
        assert_eq!(normalize_name_for_tina("simple_name"), "simplename");
        assert_eq!(
            normalize_name_for_tina("name::with::colons"),
            "namewithcolons"
        );
        assert_eq!(
            normalize_name_for_tina("name{with}braces"),
            "namewithbraces"
        );
        assert_eq!(normalize_name_for_tina("name#with#hash"), "namewithhash");
        assert_eq!(
            normalize_name_for_tina("complex_{name}#with::all"),
            "complexnamewithall"
        );
    }
}
