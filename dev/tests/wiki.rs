use capntproto_dev::{wiki, write};
use std::{collections::BTreeMap, path::Path};
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        (
            "docs/wiki/Home.md",
            "# Home\n\n[Guide](Guide.md#hello-world)\n[Source](../../src/lib.rs)\n",
        ),
        (
            "docs/wiki/Guide.md",
            "# Guide\n\n## Hello `world`!\n\n[Home](Home.md)\n",
        ),
        (
            "docs/wiki/_Sidebar.md",
            "[Home](Home.md)\n[Guide](Guide.md)\n",
        ),
        ("docs/wiki/_Footer.md", "[Home](Home.md)\n"),
        ("src/lib.rs", "// source"),
    ] {
        write(dir.path().join(name), body).unwrap();
    }
    dir
}
fn build(root: &Path) -> anyhow::Result<usize> {
    wiki::build(
        root,
        &root.join("target/wiki"),
        "owner/repository",
        "release/next",
    )
}
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_str().unwrap().into(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}
#[test]
fn export_links_and_repeat_are_deterministic() {
    let dir = fixture();
    assert_eq!(build(dir.path()).unwrap(), 4);
    let output = dir.path().join("target/wiki");
    let before = files(&output);
    let home = std::fs::read_to_string(output.join("Home.md")).unwrap();
    assert!(home.contains("https://github.com/owner/repository/wiki/Guide#hello-world"));
    assert!(home.contains("https://github.com/owner/repository/blob/release%2Fnext/src/lib.rs"));
    build(dir.path()).unwrap();
    assert_eq!(files(&output), before);
    assert!(
        std::fs::read_to_string(dir.path().join("docs/wiki/Home.md"))
            .unwrap()
            .contains("Guide.md#hello-world")
    );
}
#[test]
fn broken_destinations_and_anchors_stop_export() {
    let dir = fixture();
    write(
        dir.path().join("docs/wiki/Guide.md"),
        "# Guide\n[Missing](missing.md)\n[Bad](Home.md#missing)",
    )
    .unwrap();
    let error = wiki::check(dir.path()).unwrap_err().to_string();
    assert!(
        error.contains("missing target missing.md")
            && error.contains("missing anchor Home.md#missing")
    );
    assert!(build(dir.path()).is_err());
    assert!(!dir.path().join("target/wiki").exists());
}
#[test]
fn markdown_code_is_not_followed() {
    let text="```md\n[Fake](missing.md)\n```\n~~~\n[Fake](missing.md)\n~~~\n`[Fake](missing.md)`\n    [Fake](missing.md)\n<!-- [Fake](missing.md) -->\n[Real](Guide.md)\n![Image](../../image.svg)\n[ref]: <Home.md>\n";
    assert_eq!(
        wiki::links(text)
            .into_iter()
            .map(|(_, u)| u)
            .collect::<Vec<_>>(),
        ["Guide.md", "../../image.svg", "Home.md"]
    );
}
#[test]
fn github_heading_anchors_include_collisions() {
    assert_eq!(
        wiki::anchors("# Hello `world`!\n## Hello world\n## Hello world-1\nTitle\n=====\n"),
        ["hello-world", "hello-world-1", "hello-world-1-1", "title"]
            .map(str::to_owned)
            .into()
    );
}
#[test]
fn unrelated_and_modified_exports_are_preserved() {
    for (name, body, build_first) in [
        ("notes.md", "human work", false),
        ("Guide.md", "independent wiki edit", true),
        ("notes.txt", "notes", true),
    ] {
        let dir = fixture();
        if build_first {
            build(dir.path()).unwrap();
        }
        let file = dir.path().join("target/wiki").join(name);
        write(&file, body).unwrap();
        assert!(build(dir.path()).is_err());
        assert_eq!(std::fs::read_to_string(file).unwrap(), body);
    }
}
#[test]
fn only_stale_owned_exports_are_removed() {
    let dir = fixture();
    build(dir.path()).unwrap();
    std::fs::remove_file(dir.path().join("docs/wiki/Guide.md")).unwrap();
    write(dir.path().join("docs/wiki/Home.md"), "# Home\n").unwrap();
    write(dir.path().join("docs/wiki/_Sidebar.md"), "[Home](Home.md)").unwrap();
    assert_eq!(build(dir.path()).unwrap(), 3);
    assert!(!dir.path().join("target/wiki/Guide.md").exists());
}
#[test]
fn source_output_is_rejected() {
    let dir = fixture();
    assert!(
        wiki::build(dir.path(), &dir.path().join("docs/wiki"), "a/b", "main")
            .unwrap_err()
            .to_string()
            .contains("source documents")
    );
}
#[cfg(unix)]
#[test]
fn symlink_output_is_rejected() {
    let dir = fixture();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(dir.path().join("docs/wiki"), &alias).unwrap();
    assert!(wiki::build(dir.path(), &alias, "a/b", "main")
        .unwrap_err()
        .to_string()
        .contains("symlinks"));
}
#[test]
fn navigation_case_collisions_and_new_documents_are_checked() {
    let dir = fixture();
    write(
        dir.path().join("docs/wiki/guide.md"),
        "# lowercase collision",
    )
    .unwrap();
    write(dir.path().join("new.md"), "[broken](missing.md)").unwrap();
    let error = wiki::check(dir.path()).unwrap_err().to_string();
    for expected in [
        "duplicate wiki page name",
        "absent from sidebar",
        "missing target",
    ] {
        assert!(error.contains(expected));
    }
}
#[test]
fn repository_escape_fails_but_build_outputs_are_optional() {
    let dir = fixture();
    write(dir.path().join("docs/wiki/Home.md"),"# Home\n[Escape](../../../../secret.md)\n[Generated](../../target/report.json)\n[Missing](../../research/missing.json)").unwrap();
    let error = wiki::check(dir.path()).unwrap_err().to_string();
    assert!(error.contains("escapes repository"));
    assert!(!error.contains("target/report.json"));
    assert!(error.contains("research/missing.json"));
}
