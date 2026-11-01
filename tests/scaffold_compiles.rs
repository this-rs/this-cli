//! The chain nothing used to test: **`this init` produces a project that COMPILES.**
//!
//! `this add target desktop` shipped for months with eight independent defects that made the
//! generated crate impossible to build — a missing library target, a call to `into_router()`
//! that does not exist, an `await` on a synchronous constructor, a dependency path one level
//! short, a Tauri config Tauri 2 rejects, an empty `icons/`. Rendering a template and asserting
//! on substrings proves the template renders; it says nothing about the result being a program.
//!
//! Ignored by default: it builds the generated project, so it needs the network on a cold
//! registry and a few minutes. CI runs it explicitly with `--ignored`.

use std::path::Path;
use std::process::Command;

fn cli(args: &[&str], cwd: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_this"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run this");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), all)
}

fn cargo(args: &[&str], cwd: &Path, target: &Path) -> std::process::Output {
    Command::new(env!("CARGO"))
        .args(args)
        .current_dir(cwd)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("run cargo")
}

#[test]
#[ignore = "builds the generated project (network + minutes); CI runs it with --ignored"]
fn a_scaffolded_project_compiles() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let root = tmp.path();
    let target = root.join(".smoke-target");

    // Both shapes `init` can produce.
    for (name, flags) in [("classic", vec![]), ("ws", vec!["--workspace"])] {
        let mut args = vec!["init", name, "--no-git"];
        args.extend(flags);
        let (code, out) = cli(&args, root);
        assert_eq!(code, 0, "`this init {name}` failed:\n{out}");
    }

    // The classic scaffold: the binary, and the library it now imports.
    let out = cargo(&["check", "--quiet"], &root.join("classic"), &target);
    assert!(
        out.status.success(),
        "the classic scaffold does not compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The workspace scaffold: `api/` carries the library the desktop target consumes.
    let out = cargo(&["check", "--quiet"], &root.join("ws").join("api"), &target);
    assert!(
        out.status.success(),
        "the workspace `api/` does not compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The desktop target. `cargo metadata` only resolves the graph — it proves the dependency
    // path and the manifest, not that the crate builds (that needs Tauri's system libraries).
    let (code, out) = cli(
        &["add", "target", "webapp", "--framework", "react"],
        &root.join("ws"),
    );
    assert_eq!(code, 0, "`this add target webapp` failed:\n{out}");
    let (code, out) = cli(&["add", "target", "desktop"], &root.join("ws"));
    assert_eq!(code, 0, "`this add target desktop` failed:\n{out}");

    let src_tauri = root
        .join("ws")
        .join("targets")
        .join("desktop")
        .join("src-tauri");
    assert!(
        src_tauri.join("icons").join("icon.png").is_file(),
        "the desktop target must ship a real icon: `tauri::generate_context!` opens it at \
         compile time and refuses anything that is not RGBA"
    );
    let out = cargo(
        &["metadata", "--format-version", "1", "--no-deps"],
        &src_tauri,
        &target,
    );
    assert!(
        out.status.success(),
        "the desktop crate does not resolve its dependencies:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
