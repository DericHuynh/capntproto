//! Locale-independent equivalent of the pinned KJ `%g` round-trip formatting.
pub(crate) fn float64(value: f64) -> String {
    if !value.is_finite() {
        return special(value);
    }
    let short = general(value, 15);
    if short.parse::<f64>().ok() == Some(value) {
        short
    } else {
        general(value, 17)
    }
}
pub(crate) fn float32(value: f32) -> String {
    if !value.is_finite() {
        return special(value.into());
    }
    let short = general(value.into(), 6);
    if short.parse::<f32>().ok() == Some(value) {
        short
    } else {
        general(value.into(), 8)
    }
}
fn special(value: f64) -> String {
    if value.is_nan() {
        "nan".into()
    } else if value.is_sign_negative() {
        "-inf".into()
    } else {
        "inf".into()
    }
}
fn trim(mut s: String) -> String {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}
fn general(value: f64, digits: usize) -> String {
    let scientific = format!("{:.*e}", digits - 1, value);
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent = exponent.parse::<i32>().unwrap();
    if exponent < -4 || exponent >= digits as i32 {
        format!(
            "{}e{}{:02}",
            trim(mantissa.into()),
            if exponent < 0 { "-" } else { "" },
            exponent.unsigned_abs()
        )
    } else {
        trim(format!(
            "{:.*}",
            (digits as i32 - 1 - exponent).max(0) as usize,
            value
        ))
    }
}
