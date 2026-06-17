use std::process::Command;

fn main() {
    // Emit version constants into $OUT_DIR/version.rs, which src/cli/parser.rs
    // pulls in via include!. Keeping the generated file in build output (not
    // the source tree) means rustfmt never has to resolve a missing module and
    // `cargo fmt --check` works on a fresh checkout / in CI without a prior
    // build. version.sh writes to the path passed as $1.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = format!("{out_dir}/version.rs");
    Command::new("./version.sh")
        .arg(&dest)
        .status()
        .expect("git version shell script should succeed");
    println!("cargo:rerun-if-changed=.git/HEAD");
}
