use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=DTP_BUILD_REVISION");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let head = root.join(".git/HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    if let Ok(reference) = fs::read_to_string(&head)
        && let Some(reference) = reference.trim().strip_prefix("ref: ")
    {
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git").join(reference).display()
        );
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git/packed-refs").display()
        );
    }
    let revision = env::var("DTP_BUILD_REVISION")
        .ok()
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into());
    let safe = if revision.len() == 40 && revision.chars().all(|c| c.is_ascii_hexdigit()) {
        revision
    } else {
        "unknown".into()
    };
    println!("cargo:rustc-env=DTP_BUILD_REVISION={safe}");
}
