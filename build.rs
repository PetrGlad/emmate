use std::process::Command;
fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("parse current git revision");
    let git_hash = String::from_utf8(output.stdout).expect("parse output as UTF8");

    println!("cargo:rustc-rerun-if-changed=.git/HEAD");
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
