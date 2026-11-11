//! Compile the optional macOS bridge at build time; no compiler is needed at runtime.
use std::{env,path::PathBuf,process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/macos/voice.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("macos") || env::var_os("CARGO_FEATURE_APP").is_none(){return;}
    let out=PathBuf::from(env::var_os("OUT_DIR").unwrap());let object=out.join("voice.o");
    let arch=if env::var("CARGO_CFG_TARGET_ARCH").as_deref()==Ok("aarch64"){"arm64"}else{"x86_64"};
    let result=Command::new("xcrun").args(["clang","-fobjc-arc","-fblocks","-O2","-Wall","-Wextra","-Werror","-mmacosx-version-min=11.0","-arch",arch,"-c","src/macos/voice.m","-o"]).arg(&object).output().expect("Xcode command line tools are required for the macOS voice bridge");
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let archive=out.join("libruru_voice.a");
    assert!(Command::new("xcrun").args(["ar","crs"]).arg(&archive).arg(object).status().unwrap().success());
    println!("cargo:rustc-link-search=native={}",out.display());println!("cargo:rustc-link-lib=static=ruru_voice");
    for framework in ["Foundation","AVFoundation","Speech"] {println!("cargo:rustc-link-lib=framework={framework}");}
}
