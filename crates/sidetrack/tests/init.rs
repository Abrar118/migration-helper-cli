use std::process::Command;

#[test]
fn compose_files_are_relative_to_the_command_directory() {
    if !sidetrack_core::compose::docker_available() {
        return;
    }
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
