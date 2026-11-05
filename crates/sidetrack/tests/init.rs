use std::process::Command;

fn project_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let infra = root.join("infra");
    std::fs::create_dir(&infra).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        infra.join("dev.yml"),
        r#"name: sidetrack-cli-path-test
services:
  db:
    image: postgres:16
    environment: ["POSTGRES_DB=shop"]
  api:
    image: postgres:16
    depends_on: [db]
    environment: ["DATABASE_URL=${DATABASE_URL}"]
"#,
    )
    .unwrap();
    std::fs::write(infra.join(".env"), "DATABASE_URL=postgresql://db/shop\n").unwrap();
    std::fs::write(
        root.join("alembic.ini"),
        "[alembic]\nscript_location = alembic\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("alembic/versions")).unwrap();
    dir
}

#[test]
fn compose_files_are_relative_to_the_command_directory() {
    if !sidetrack_core::compose::docker_available() {
        return;
    }
    let dir = project_fixture();
    let root = dir.path().canonicalize().unwrap();
    let infra = root.join("infra");
    for (cwd, args) in [(&infra, vec![]), (&root, vec!["-C", "infra"])] {
        let output = Command::new(env!("CARGO_BIN_EXE_sidetrack"))
            .current_dir(cwd)
            .args(args)
            .args(["init", "-f", "dev.yml", "--dry-run", "--no-hook"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("infra/dev.yml"));
        assert!(!root.join("sidetrack.toml").exists());
    }
}

#[test]
fn external_compose_paths_warn_even_with_yes() {
    if !sidetrack_core::compose::docker_available() {
        return;
    }
    let dir = project_fixture();
    let root = dir.path().canonicalize().unwrap();
    let external = tempfile::tempdir().unwrap();
    let file = external.path().join("dev.yml");
    std::fs::copy(root.join("infra/dev.yml"), &file).unwrap();
    std::fs::copy(root.join("infra/.env"), external.path().join(".env")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sidetrack"))
        .current_dir(&root)
        .args(["init", "-f"])
        .arg(&file)
        .args(["--yes", "--dry-run", "--no-hook"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("absolute Compose paths"));
    assert!(!root.join("sidetrack.toml").exists());
}
