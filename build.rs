//! Records the git commit in the binary (`HARROW_GIT_COMMIT`), so networked
//! players can check they run the same build. "unknown" without git.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    // Builds without the git repository (in a container, on a server from a
    // source archive) are told the commit instead.
    println!("cargo:rerun-if-env-changed=HARROW_BUILD_COMMIT");
    println!("cargo:rerun-if-changed=build.rs");
    let id = match std::env::var("HARROW_BUILD_COMMIT") {
        Ok(id) if !id.trim().is_empty() => {
            // Told: don't watch the repository (commits needn't rebuild).
            println!("cargo:rustc-env=HARROW_GIT_COMMIT={}", id.trim());
            return;
        }
        _ => {
            let commit = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "unknown".into());
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|s| !s.is_empty());
            if dirty { format!("{commit}-dirty") } else { commit }
        }
    };
    println!("cargo:rustc-env=HARROW_GIT_COMMIT={id}");
    // Re-run when HEAD moves (commits, checkouts) or the index changes.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/index");
        if let (Some(common), Some(head_ref)) = (
            git(&["rev-parse", "--git-common-dir"]),
            git(&["symbolic-ref", "-q", "HEAD"]),
        ) {
            println!("cargo:rerun-if-changed={common}/{head_ref}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
