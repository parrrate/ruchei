use std::process::Command;

fn main() {
    let token = std::env::var("INPUT_TOKEN").unwrap();
    let ref_name = std::env::var("GITHUB_REF_NAME").unwrap();
    let package = ref_name.split_once('/').map(|(n, _)| n);
    let mut cmd = Command::new("cargo");
    cmd.arg("publish");
    if let Some(package) = package {
        cmd.arg("--package").arg(package);
    }
    cmd.env("CARGO_REGISTRY_TOKEN", token);
    assert!(cmd.status().unwrap().success());
}
