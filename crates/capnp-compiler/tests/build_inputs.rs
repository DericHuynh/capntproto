use capnp_compiler::FileCompiler;
use std::{collections::BTreeSet, fs, path::Path};

fn expected(paths: &[&Path]) -> Vec<std::path::PathBuf> {
    paths
        .iter()
        .flat_map(|p| [std::path::absolute(p).unwrap(), p.canonicalize().unwrap()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn filesystem_compilation_reports_loaded_sources_and_embeds_per_invocation() {
    let directory = tempfile::tempdir().unwrap();
    // Relative imports resolve beside the physical importing file. macOS /var
    // and Windows short temporary paths can themselves be aliases.
    let directory_path = directory.path().canonicalize().unwrap();
    let main = directory_path.join("main.capnp");
    let blob = directory_path.join("blob");
    let first = directory_path.join("first");
    let second = directory_path.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let source = "@0xbbbbbbbbbbbbbbbb; struct Item { x @0 :Text; } using Missing = import \"missing.capnp\"; const unused :Data = embed \"unused\";";
    fs::write(first.join("dep.capnp"), source).unwrap();
    fs::write(second.join("dep.capnp"), source).unwrap();
    fs::write(&main, "@0xabcdefabcdefabcd; struct S { item @0 :import \"/dep.capnp\".Item; data @1 :Data = embed \"blob\"; }").unwrap();
    let mut compiler = FileCompiler::new();
    compiler
        .src_prefix(directory.path())
        .import_path(&first)
        .import_path(&second);
    assert!(compiler.compile_with_dependencies(&[&main]).is_err());
    fs::write(&blob, [0, 255]).unwrap();
    let initial = compiler.compile_with_dependencies(&[&main]).unwrap();
    assert_eq!(
        initial.dependencies,
        expected(&[&main, &blob, &first.join("dep.capnp")])
    );
    let before = capnp::serialize::write_message_to_words(&initial.message);
    fs::write(&blob, [1, 2, 3]).unwrap();
    fs::remove_file(first.join("dep.capnp")).unwrap();
    let updated = compiler.compile_with_dependencies(&[&main]).unwrap();
    assert_eq!(
        updated.dependencies,
        expected(&[&main, &blob, &second.join("dep.capnp")])
    );
    assert_ne!(
        before,
        capnp::serialize::write_message_to_words(&updated.message)
    );
}

#[cfg(unix)]
#[test]
fn dependency_paths_retain_symlinks_and_their_resolved_targets() {
    let directory = tempfile::tempdir().unwrap();
    let directory_path = directory.path().canonicalize().unwrap();
    let main = directory_path.join("main.capnp");
    let dep = directory_path.join("dep.capnp");
    let alias = directory_path.join("alias.capnp");
    fs::write(&main, "@0xabcdefabcdefabcd; struct S { a @0 :import \"alias.capnp\".Item; b @1 :import \"dep.capnp\".Item; }").unwrap();
    fs::write(&dep, "@0xbbbbbbbbbbbbbbbb; struct Item {}").unwrap();
    std::os::unix::fs::symlink(&dep, &alias).unwrap();
    let compiled = FileCompiler::new()
        .src_prefix(directory.path())
        .compile_with_dependencies(&[&main])
        .unwrap();
    assert_eq!(compiled.dependencies, expected(&[&main, &alias, &dep]));
}
