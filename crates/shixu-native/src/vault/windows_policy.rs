//! Platform independent policy; these tests do not prove Windows behavior.
use shixu_core::contracts::{AppResult, error::AppError};
pub(super) fn quote(value: &str) -> AppResult<String> {
    if value.contains('\0') {
        return Err(AppError::InvalidInput);
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        slashes = 0;
        out.push(ch);
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    Ok(out)
}
pub(super) fn local_path(value: &str) -> AppResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":\\"
        || value.contains(['\0', '/'])
    {
        return Err(AppError::Unsupported);
    }
    for part in value[3..].split('\\') {
        if part.is_empty() && value.len() == 3 {
            continue;
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.contains([':', '<', '>', '"', '|', '?', '*'])
            || part.chars().any(|ch| ch.is_control())
            || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || (stem.chars().count() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem
                    .chars()
                    .nth(3)
                    .is_some_and(|ch| ch.is_ascii_digit() || ['¹', '²', '³'].contains(&ch)))
        {
            return Err(AppError::Unsupported);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_windows_arguments_keep_quotes_and_trailing_slashes() {
        assert_eq!(quote("C:\\空 格\\").unwrap(), "\"C:\\空 格\\\\\"");
        assert_eq!(quote("a\\\"b").unwrap(), "\"a\\\\\\\"b\"");
        assert!(quote("x\0y").is_err());
    }
    #[test]
    fn supported_local_paths_reject_aliases_and_remote_names() {
        for bad in [
            "relative",
            "\\\\host\\share",
            "\\\\?\\C:\\x",
            "C:\\x:ads",
            "C:\\..\\x",
            "C:\\CON",
            "C:\\x.",
            "C:\\x ",
            "C:/x",
            "C:\\CONIN$",
            "C:\\COM¹",
        ] {
            assert!(local_path(bad).is_err(), "{bad}");
        }
        assert!(local_path("C:\\空 格\\vault").is_ok());
    }
}
