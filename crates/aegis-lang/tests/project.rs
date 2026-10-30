use aegis_lang::{
    project::{self, Overlay},
    MAX_SOURCE,
};
use std::fs;
use zeroize::Zeroizing;

#[test]
fn loads_nested_modules_and_maps_original_spans() {
    let dir = tempfile::tempdir().expect("dir");
    fs::create_dir(dir.path().join("world")).expect("module dir");
    let entry = dir.path().join("main.ae");
    let source = "mod world; fn main() { let p=world::make(); printi(world::math::score(p)); }";
    fs::write(&entry, source).expect("entry");
    let module = "\u{feff}// привет\nmod math; struct Point { x:i64 } fn make()->Point { return Point { x:42 }; }";
    fs::write(dir.path().join("world.ae"), module).expect("module");
    let child = "fn score(p:Point) { return p.x; }";
    fs::write(dir.path().join("world/math.ae"), child).expect("child");
    let build = project::compile(&entry, source, &[]).expect("project");
    assert_eq!(build.sources.len(), 3);
    let make = build
        .functions
        .iter()
        .find(|f| f.name == "world::make")
        .expect("make");
    assert_eq!(
        &module[make.span.clone()],
        "fn make()->Point { return Point { x:42 }; }"
    );
    assert_eq!(
        make.path.as_ref(),
        Some(&dir.path().join("world.ae").canonicalize().expect("path"))
    );
    let score = build
        .functions
        .iter()
        .find(|f| f.name == "world::math::score")
        .expect("score");
    assert_eq!(&child[score.span.clone()], child);
    assert_eq!(
        score.signature,
        "world::math::score(p: world::Point) -> i64"
    );
}

#[test]
fn overlays_are_used_without_writing_plaintext() {
    let dir = tempfile::tempdir().expect("dir");
    let entry = dir.path().join("main.ae");
    let source = "mod helper; fn main() { printi(helper::value()); }";
    let path = dir.path().join("helper.ae");
    fs::write(&path, "bad disk content").expect("disk");
    let overlays = [Overlay {
        path: path.clone(),
        text: Zeroizing::new("fn value() { return 42; }".into()),
    }];
    project::compile(&entry, source, &overlays).expect("virtual entry and override");
    assert_eq!(
        fs::read_to_string(&path).expect("unchanged"),
        "bad disk content"
    );
    assert!(!entry.exists());
    fs::remove_file(&path).expect("remove test file");
    project::compile(&entry, source, &overlays).expect("not yet saved named module");
    assert!(!path.exists());
}

#[test]
fn diagnostics_belong_to_the_dependency_and_keep_unicode_columns() {
    let dir = tempfile::tempdir().expect("dir");
    let entry = dir.path().join("main.ae");
    let source = "mod helper; fn main() {}";
    let path = dir.path().join("helper.ae");
    let child = "// привет\nfn value() { printi(missing); }";
    fs::write(&path, child).expect("module");
    let error = project::compile(&entry, source, &[]).expect_err("bad module");
    assert_eq!(error.path, Some(path.canonicalize().expect("canonical")));
    assert_eq!(&child[error.span], "missing");
    assert_eq!((error.line, error.column), (2, 21));
    fs::write(&path, "fn value() {").expect("truncated");
    let error = project::compile(&entry, source, &[]).expect_err("unclosed child");
    assert_eq!(error.path, Some(path.canonicalize().expect("canonical")));
    assert_eq!(error.span.start, "fn value() {".len());
    let empty = project::compile(&entry, "", &[]).expect_err("empty entry");
    assert_eq!(
        empty.path,
        Some(dir.path().canonicalize().expect("root").join("main.ae"))
    );
    let bom = project::compile(&entry, "\u{feff}", &[]).expect_err("BOM only");
    assert_eq!(bom.span, 3..3);
}

#[test]
fn project_loader_rejects_invalid_utf8_dependency() {
    let dir = tempfile::tempdir().expect("dir");
    let entry = dir.path().join("main.ae");
    fs::write(dir.path().join("broken.ae"), [0xff, 0xfe, 0x00]).expect("invalid module");
    let error = project::compile(&entry, "mod broken; fn main() {}", &[])
        .expect_err("invalid UTF-8 dependency");
    assert!(error.message.contains("cannot load module"));
    assert_eq!(
        error.path,
        Some(
            dir.path()
                .canonicalize()
                .expect("canonical root")
                .join("main.ae")
        )
    );
}

#[test]
fn project_budgets_and_missing_modules_are_checked() {
    let dir = tempfile::tempdir().expect("dir");
    assert!(project::compile(dir.path(), "fn main() {}", &[]).is_err());

    let entry = dir.path().join("main.ae");
    assert!(project::compile(&entry, "mod missing; fn main() {}", &[])
        .expect_err("missing")
        .message
        .contains("cannot load module"));
    fs::write(
        dir.path().join("large.ae"),
        format!("//{}", "x".repeat(MAX_SOURCE - 2)),
    )
    .expect("large");
    assert!(project::compile(&entry, "mod large; fn main() {}", &[])
        .expect_err("bytes")
        .message
        .contains("exceed"));
    let mut source = String::new();
    for index in 0..63 {
        fs::write(dir.path().join(format!("m{index}.ae")), "").expect("empty module");
        source.push_str(&format!("mod m{index};"));
    }
    source.push_str("fn main() {}");
    assert_eq!(
        project::compile(&entry, &source, &[])
            .expect("64 files")
            .sources
            .len(),
        64
    );
    fs::write(dir.path().join("last.ae"), "").expect("last");
    assert!(project::compile(&entry, &format!("mod last;{source}"), &[])
        .expect_err("65 files")
        .message
        .contains("file limit"));
}

#[cfg(windows)]
#[test]
fn junctions_cannot_escape_or_cycle_module_loading() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    let entry = dir.path().join("main.ae");
    let escape = dir.path().join("escape");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&escape)
        .arg(outside.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("junction tool");
    assert!(status.success(), "create test junction");
    fs::write(outside.path().join("secret.ae"), "fn value() { return 1; }").expect("outside");
    let error = project::compile(&entry, "mod escape { mod secret; } fn main() {}", &[])
        .expect_err("escape");
    assert!(error.message.contains("outside the project root"));
    let cycle = dir.path().join("again");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&cycle)
        .arg(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("cycle junction");
    assert!(status.success());
    let source = "mod again { mod main; } fn main() {}";
    fs::write(&entry, source).expect("entry");
    assert!(project::compile(&entry, source, &[])
        .expect_err("cycle")
        .message
        .contains("cyclic module"));
}

#[test]
fn fingerprint_changes_with_entry_or_overlay_content() {
    use aegis_lang::project::{fingerprint, Overlay};
    let dir = tempfile::tempdir().expect("dir");
    let entry = dir.path().join("main.ae");
    let child = dir.path().join("child.ae");
    let overlays = vec![Overlay {
        path: child,
        text: String::from("fn value() { return 1; }").into(),
    }];
    let first = fingerprint(&entry, "fn main() {}", &overlays).expect("fingerprint");
    let second = fingerprint(&entry, "fn main() { return 1; }", &overlays).expect("fingerprint");
    assert_ne!(first, second);
}
