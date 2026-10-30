//! End-to-end `this descriptor …` / `this add module` / `this info --json`, through the
//! real binary (exit codes included).

use std::path::Path;
use std::process::Command;

fn run(args: &[&str], cwd: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_this"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run this");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn new_project(tmp: &Path) -> std::path::PathBuf {
    let (code, out, err) = run(&["init", "shop", "--no-git"], tmp);
    assert_eq!(code, 0, "init: {out}{err}");
    tmp.join("shop")
}

#[test]
fn descriptor_check_cli_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());

    // No descriptor: nothing to check, exit 0 (a project without descriptor/module.yaml
    // works as before).
    let (code, out, _) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("nothing to check"));

    let (code, out, err) = run(&["descriptor", "init"], &p);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = run(
        &[
            "add",
            "entity",
            "customer",
            "--fields",
            "email:String:required:email,age:Option<i32>:max_value(150)",
        ],
        &p,
    );
    assert_eq!(code, 0, "{out}{err}");

    let (code, out, err) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 0, "valid descriptor: {out}{err}");
    assert!(out.contains("Descriptor OK"), "{out}");

    // JSON report.
    let (code, out, _) = run(&["descriptor", "check", "--json"], &p);
    assert_eq!(code, 0);
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["descriptor"], true);
    assert_eq!(report["errors"].as_array().unwrap().len(), 0);

    // module.yaml redeclaring fields the macro owns: refused, exit code 1.
    let yaml_path = p.join("descriptor/module.yaml");
    let good = std::fs::read_to_string(&yaml_path).unwrap();
    std::fs::write(
        &yaml_path,
        good.replace(
            "- name: customer",
            "- name: customer\n  fields:\n  - { name: email, ty: string }",
        ),
    )
    .unwrap();
    let (code, out, err) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 1, "declared twice must fail: {out}{err}");
    assert!(
        format!("{out}{err}").contains("this.descriptor.declared_twice"),
        "{out}{err}"
    );

    // Unknown policy: refused at parse, exit code 1.
    std::fs::write(
        &yaml_path,
        good.replace(
            "- name: customer",
            "- name: customer\n  permissions: { list: publik }",
        ),
    )
    .unwrap();
    let (code, _, _) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 1);

    // Back to valid; a tag moved by hand in the lock-protected JSON is caught via an
    // explicit proto_tag in the... lock itself: two fields on one tag.
    std::fs::write(&yaml_path, &good).unwrap();
    let lock_path = p.join("descriptor/descriptor.lock");
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    std::fs::write(&lock_path, lock.replace("age: 17", "age: 16")).unwrap();
    let (code, out, err) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 1, "tag reuse must fail: {out}{err}");
    assert!(format!("{out}{err}").contains("tag_reused"));
    std::fs::write(&lock_path, lock).unwrap();

    // Entity missing from Module::descriptor(): the server would compose it without
    // fields nor rules while module.descriptor.json has them. Exit code 1.
    let module_path = p.join("src/module.rs");
    let module_rs = std::fs::read_to_string(&module_path).unwrap();
    let registered = "crate::entities::customer::model::described(),";
    assert!(module_rs.contains(registered), "{module_rs}");
    std::fs::write(&module_path, module_rs.replace(registered, "")).unwrap();
    let (code, out, err) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 1, "unregistered entity must fail: {out}{err}");
    assert!(
        format!("{out}{err}").contains("missing from Module::descriptor()"),
        "{out}{err}"
    );
    std::fs::write(&module_path, module_rs).unwrap();

    // Stale JSON: warning only, unless --strict.
    std::fs::write(p.join("descriptor/module.descriptor.json"), "{}").unwrap();
    let (code, _, _) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 0);
    let (code, _, _) = run(&["descriptor", "check", "--strict"], &p);
    assert_eq!(code, 1);
    let (code, _, _) = run(&["descriptor", "export"], &p);
    assert_eq!(code, 0);
    let (code, _, _) = run(&["descriptor", "check", "--strict"], &p);
    assert_eq!(code, 0);
}

#[test]
fn add_module_requires_manifest_and_info_json_exports_descriptor() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    assert_eq!(run(&["descriptor", "init"], &p).0, 0);
    assert_eq!(run(&["add", "entity", "order"], &p).0, 0);

    // Manifest of another module (its materialized descriptor).
    let manifest = tmp.path().join("users.descriptor.json");
    std::fs::write(
        &manifest,
        r#"{"format":1,"name":"users","version":"1.4.2","entities":[{"name":"user","fields":[{"name":"email","ty":"string"}],"permissions":{"list":"authenticated","get":"authenticated","create":"authenticated","update":"authenticated","delete":"admin_only"}}]}"#,
    )
    .unwrap();
    let (code, out, err) = run(&["add", "module", manifest.to_str().unwrap()], &p);
    assert_eq!(code, 0, "{out}{err}");
    let yaml = std::fs::read_to_string(p.join("descriptor/module.yaml")).unwrap();
    assert!(yaml.contains("name: users"), "{yaml}");
    assert!(yaml.contains("^1.4.2"), "{yaml}");
    assert!(p.join("descriptor/modules/users.json").is_file());

    // A link towards the required module's entity is not "undeclared".
    let (code, out, err) = run(&["add", "link", "order", "user"], &p);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = run(&["descriptor", "check"], &p);
    assert_eq!(code, 0, "{out}{err}");

    // A broken manifest is refused.
    std::fs::write(&manifest, r#"{"format":1,"name":"Bad Name","version":"x"}"#).unwrap();
    let (code, _, _) = run(&["add", "module", manifest.to_str().unwrap()], &p);
    assert_eq!(code, 1);

    // info --json carries the materialized descriptor.
    let (code, out, err) = run(&["info", "--json"], &p);
    assert_eq!(code, 0, "{out}{err}");
    let info: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(info["descriptor"]["name"], "shop");
    assert_eq!(info["descriptor"]["entities"][0]["name"], "order");
    assert_eq!(info["descriptor"]["requires"][0]["name"], "users");
    assert_eq!(info["descriptor"]["links"][0]["target"], "user");
}

#[test]
fn descriptor_export_formats_proto_graphql_json_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    assert_eq!(run(&["descriptor", "init"], &p).0, 0);
    let (code, out, err) = run(
        &[
            "add",
            "entity",
            "customer",
            "--fields",
            "email:String:required:email,age:Option<i32>:max_value(150)",
        ],
        &p,
    );
    assert_eq!(code, 0, "{out}{err}");

    // The generated Module::descriptor() applies descriptor.lock (stable proto tags at run time).
    let module_rs = std::fs::read_to_string(p.join("src/module.rs")).unwrap();
    assert!(
        module_rs.contains("compose_locked(&refs, lock)"),
        "{module_rs}"
    );
    assert!(
        module_rs.contains("descriptor/descriptor.lock"),
        "{module_rs}"
    );
    // The generated list handler reads ListQuery (REST filters are applied).
    let handlers = std::fs::read_to_string(p.join("src/entities/customer/handlers.rs")).unwrap();
    assert!(handlers.contains("ListQuery"), "{handlers}");

    // proto: the locked tags (email 16, age 17) appear.
    let (code, proto, err) = run(
        &["descriptor", "export", "--format", "proto", "--stdout"],
        &p,
    );
    assert_eq!(code, 0, "{proto}{err}");
    assert!(proto.contains("syntax = \"proto3\""), "{proto}");
    assert!(proto.contains("email = 16;"), "{proto}");
    assert!(proto.contains("age = 17;"), "{proto}");

    // graphql
    let (code, sdl, err) = run(
        &["descriptor", "export", "--format", "graphql", "--stdout"],
        &p,
    );
    assert_eq!(code, 0, "{sdl}{err}");
    assert!(sdl.contains("Customer"), "{sdl}");
    assert!(sdl.contains("email"), "{sdl}");

    // json-schema: create/update per entity; the integer bound stays an integer.
    let (code, js, err) = run(
        &[
            "descriptor",
            "export",
            "--format",
            "json-schema",
            "--stdout",
        ],
        &p,
    );
    assert_eq!(code, 0, "{js}{err}");
    let v: serde_json::Value = serde_json::from_str(&js).unwrap();
    assert!(v.get("customer.create").is_some(), "{js}");
    assert!(v.get("customer.update").is_some(), "{js}");
    assert_eq!(
        v["customer.create"]["properties"]["age"]["anyOf"][0]["maximum"], 150,
        "{js}"
    );

    // Unknown format: exit code 1.
    let (code, _, _) = run(
        &["descriptor", "export", "--format", "nope", "--stdout"],
        &p,
    );
    assert_eq!(code, 1);
}

fn ok(args: &[&str], cwd: &Path) -> String {
    let (code, out, err) = run(args, cwd);
    assert_eq!(code, 0, "this {args:?}: {out}{err}");
    format!("{out}{err}")
}

#[test]
fn descriptor_init_counts_entities_and_refuses_second_init() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    ok(&["add", "entity", "customer"], &p);

    let out = ok(&["descriptor", "init"], &p);
    assert!(out.contains("(1 entity)"), "{out}");
    let yaml = std::fs::read_to_string(p.join("descriptor/module.yaml")).unwrap();
    assert!(yaml.contains("name: customer"), "{yaml}");
    // Existing entities are registered in Module::descriptor().
    let module_rs = std::fs::read_to_string(p.join("src/module.rs")).unwrap();
    assert!(
        module_rs.contains("crate::entities::customer::Customer::descriptor().clone(),"),
        "{module_rs}"
    );

    let (code, out, err) = run(&["descriptor", "init"], &p);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("descriptor/module.yaml already exists"),
        "{out}{err}"
    );

    // Two entities: plural form.
    let tmp2 = tempfile::tempdir().unwrap();
    let p2 = new_project(tmp2.path());
    ok(&["add", "entity", "customer"], &p2);
    ok(&["add", "entity", "invoice"], &p2);
    let out = ok(&["descriptor", "init"], &p2);
    assert!(out.contains("(2 entities)"), "{out}");
}

#[test]
fn descriptor_init_without_module_rs_warns_and_names_from_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    std::fs::remove_file(p.join("src/module.rs")).unwrap();

    let out = ok(&["descriptor", "init"], &p);
    assert!(out.contains("src/module.rs not found"), "{out}");
    assert!(out.contains("(0 entities)"), "{out}");
    let yaml = std::fs::read_to_string(p.join("descriptor/module.yaml")).unwrap();
    assert!(yaml.contains("name: shop"), "{yaml}");
    assert!(yaml.contains("version: 1.0.0"), "{yaml}");
    assert!(!p.join("src/module.rs").exists());
}

#[test]
fn descriptor_lock_reports_changes_then_up_to_date() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    ok(&["descriptor", "init"], &p);
    ok(
        &[
            "add",
            "entity",
            "customer",
            "--fields",
            "email:String:required",
        ],
        &p,
    );
    let lock_path = p.join("descriptor/descriptor.lock");
    assert!(lock_path.is_file());

    let out = ok(&["descriptor", "lock"], &p);
    assert!(out.contains("descriptor.lock is up to date"), "{out}");

    // Lost lock: tags are re-assigned and reported.
    std::fs::remove_file(&lock_path).unwrap();
    let out = ok(&["descriptor", "lock"], &p);
    assert!(out.contains("Updated descriptor/descriptor.lock"), "{out}");
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    assert!(lock.contains("email"), "{lock}");
    let out = ok(&["descriptor", "lock"], &p);
    assert!(out.contains("descriptor.lock is up to date"), "{out}");
}

#[test]
fn descriptor_export_and_lock_without_descriptor_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    for args in [
        &["descriptor", "export", "--stdout"][..],
        &["descriptor", "lock"][..],
    ] {
        let (code, out, err) = run(args, &p);
        assert_eq!(code, 1, "{args:?}: {out}{err}");
        assert!(
            err.contains("no descriptor/module.yaml: run `this descriptor init` first"),
            "{args:?}: {err}"
        );
    }
}

#[test]
fn descriptor_export_output_writes_then_updates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    ok(&["descriptor", "init"], &p);
    ok(
        &[
            "add",
            "entity",
            "customer",
            "--fields",
            "email:String:required",
        ],
        &p,
    );

    // Relative path, new file.
    let out = ok(
        &[
            "descriptor",
            "export",
            "--format",
            "proto",
            "--output",
            "out/shop.proto",
        ],
        &p,
    );
    assert!(out.contains("Wrote"), "{out}");
    assert!(out.contains("shop.proto"), "{out}");
    let proto = std::fs::read_to_string(p.join("out/shop.proto")).unwrap();
    assert!(proto.contains("syntax = \"proto3\""), "{proto}");
    assert!(proto.contains("email = 16;"), "{proto}");

    // Existing file, absolute path: replaced by the new rendering.
    let abs = tmp.path().join("schema.json");
    std::fs::write(&abs, "stale").unwrap();
    ok(
        &[
            "descriptor",
            "export",
            "--format",
            "json-schema",
            "--output",
            abs.to_str().unwrap(),
        ],
        &p,
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&abs).unwrap()).unwrap();
    assert!(v.get("customer.create").is_some(), "{v}");

    // yaml format to stdout.
    let yaml = ok(
        &["descriptor", "export", "--format", "yaml", "--stdout"],
        &p,
    );
    assert!(yaml.contains("name: shop"), "{yaml}");
    assert!(yaml.contains("customer"), "{yaml}");
}

#[test]
fn add_entity_in_descriptor_mode_dry_run_existing_entry_and_no_module_rs() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    ok(&["descriptor", "init"], &p);

    // Dry run: nothing written to the descriptor.
    ok(&["--dry-run", "add", "entity", "order"], &p);
    let yaml = std::fs::read_to_string(p.join("descriptor/module.yaml")).unwrap();
    assert!(!yaml.contains("order"), "{yaml}");
    assert!(!p.join("src/entities/order").exists());

    // Entity already listed in module.yaml (hand-written) and no src/module.rs.
    let yaml_path = p.join("descriptor/module.yaml");
    std::fs::write(
        &yaml_path,
        "format: 1\nname: shop\nversion: 1.0.0\nentities:\n  - name: order\n    permissions: { list: public }\n",
    )
    .unwrap();
    std::fs::remove_file(p.join("src/module.rs")).unwrap();
    ok(&["add", "entity", "order"], &p);
    let yaml = std::fs::read_to_string(&yaml_path).unwrap();
    assert_eq!(yaml.matches("name: order").count(), 1, "{yaml}");
    assert!(yaml.contains("list: public"), "{yaml}");
    assert!(p.join("descriptor/module.descriptor.json").is_file());
}

#[test]
fn add_link_to_unknown_entity_warns_and_info_reports_invalid_descriptor() {
    let tmp = tempfile::tempdir().unwrap();
    let p = new_project(tmp.path());
    ok(&["descriptor", "init"], &p);
    ok(&["add", "entity", "order"], &p);

    let out = ok(&["add", "link", "order", "ghost"], &p);
    assert!(
        out.contains("'ghost' is not an entity of this module"),
        "{out}"
    );
    assert!(!out.contains("'order' is not an entity"), "{out}");

    // Broken module.yaml: info still works and carries the error.
    std::fs::write(
        p.join("descriptor/module.yaml"),
        "format: 1\nname: shop\nversion: 1.0.0\nentities:\n  - name: order\n    permissions: { list: publik }\n",
    )
    .unwrap();
    let out = ok(&["info", "--json"], &p);
    let info: serde_json::Value = serde_json::from_str(&out).unwrap();
    let err = info["descriptor_error"].as_str().expect("descriptor_error");
    assert!(
        err.contains("publik") || err.contains("descriptor"),
        "{err}"
    );
    assert!(info["descriptor"].is_null(), "{info}");
}
