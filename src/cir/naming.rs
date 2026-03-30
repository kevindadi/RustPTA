//! Function name extraction from Petri-net place names and SID prefix abbreviations.

/// Strip `fn_` prefix and `_start` / `_end` / `_bbN` suffix from a place name.
pub fn extract_function_name(place_name: &str) -> Option<String> {
    let s = place_name.strip_prefix("fn_")?;
    if s.ends_with("_start") {
        return Some(s[..s.len() - 6].to_string());
    }
    if s.ends_with("_end") {
        return Some(s[..s.len() - 4].to_string());
    }
    if let Some(pos) = s.rfind("_bb") {
        let rest = &s[pos + 3..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(s[..pos].to_string());
        }
    }
    None
}

/// Short prefix for statement ids (e.g. `worker` → `w`).
pub fn abbreviate_function_name(name: &str) -> String {
    let mut used = std::collections::BTreeSet::new();
    abbreviate_with_registry(name, &mut used)
}

fn abbreviate_with_registry(name: &str, used: &mut std::collections::BTreeSet<String>) -> String {
    let candidates: Vec<String> = match name {
        "main" => vec!["main".into()],
        "worker" => vec!["w".into()],
        "notifier" => vec!["n".into()],
        "producer" => vec!["p".into()],
        "consumer" => vec!["c".into()],
        _ if name.starts_with("closure_") => {
            let n = name.trim_start_matches("closure_");
            vec![format!("cl{n}"), format!("c{n}")]
        }
        _ => {
            let chars: Vec<char> = name.chars().collect();
            if chars.len() >= 2 {
                vec![format!("{}{}", chars[0], chars[1])]
            } else if let Some(c) = chars.first() {
                vec![c.to_string()]
            } else {
                vec!["f".into()]
            }
        }
    };
    for c in candidates {
        if !used.contains(&c) {
            used.insert(c.clone());
            return c;
        }
    }
    let mut i = 0u32;
    loop {
        let s = format!("f{i}");
        if !used.contains(&s) {
            used.insert(s.clone());
            return s;
        }
        i += 1;
    }
}
