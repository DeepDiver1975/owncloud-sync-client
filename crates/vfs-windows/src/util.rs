/// Convert a UTF-8 string to a null-terminated wide (UTF-16) Vec<u16>.
///
/// The trailing `\0` is appended so the result can be passed to Win32 APIs
/// expecting a `LPCWSTR`.
pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0u16)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_wide_null_ascii() {
        let wide = to_wide_null("hello");
        assert_eq!(wide, &[b'h' as u16, b'e' as u16, b'l' as u16, b'l' as u16, b'o' as u16, 0]);
    }

    #[test]
    fn to_wide_null_empty() {
        let wide = to_wide_null("");
        assert_eq!(wide, &[0u16]);
    }

    #[test]
    fn to_wide_null_unicode() {
        let wide = to_wide_null("ñ");
        assert_eq!(wide[0], 0x00F1u16);
        assert_eq!(*wide.last().unwrap(), 0u16);
    }
}
