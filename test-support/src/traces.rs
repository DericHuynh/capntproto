//! Versioned model traces. Old array envelopes are deliberately unsupported.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File<T> {
    version: u32,
    model: String,
    cases: Vec<T>,
}
pub fn read<T: serde::de::DeserializeOwned>(
    path: impl AsRef<std::path::Path>,
    model: &str,
) -> Vec<T> {
    let file: File<T> = serde_json::from_slice(&std::fs::read(path).expect("read traces"))
        .expect("invalid trace schema");
    assert_eq!(file.version, 1, "unsupported trace version");
    assert_eq!(file.model, model, "wrong model input");
    assert!(!file.cases.is_empty(), "empty trace set");
    file.cases
}
