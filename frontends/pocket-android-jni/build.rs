// Build glue for `pocket-android-jni` cross-compiles.
//
// Two Android-specific link fixups happen here:
//
// 1. **`-lpthread` stub.** `unicorn-engine-sys`'s `build.rs` unconditionally
//    emits `cargo:rustc-link-lib=pthread` for any non-MSVC target. Android's
//    bionic libc provides the pthread API directly — there is no separate
//    `libpthread.so` shipped with the NDK — so the linker would fail with
//    `unable to find library -lpthread`. To satisfy the `-lpthread` flag
//    without touching the upstream crate, drop an empty `libpthread.a`
//    (8-byte `ar` magic only) next to a `cargo:rustc-link-search` path.
//    `ld.lld` accepts that as a valid archive contributing zero symbols and
//    the actual `pthread_*` symbols are resolved against `libc.so` at load.
//
// 2. **`libclang_rt.builtins-<arch>-android.a` linkage.** AArch64/ARM TCG in
//    the QEMU vendored by `unicorn-engine-sys` calls `__builtin___clear_cache`
//    after generating each translation block. Clang lowers that builtin to a
//    libcall to `__clear_cache`, which is *not* exported by bionic libc — it
//    lives in `libclang_rt.builtins-<arch>-android.a` (NDK's compiler-rt).
//    Rust's cross link does not pull `--rtlib=compiler-rt` automatically when
//    cargo-ndk drives the link, so the resulting `libpockethle_jni.so` ends
//    up with `__clear_cache` as an *undefined dynamic symbol*. With BIND_NOW
//    on (Android default), `dlopen()` then fails on the user's device with
//    `cannot locate symbol "__clear_cache"`, `System.loadLibrary` throws
//    `UnsatisfiedLinkError`, and the app crashes the moment it starts —
//    which is exactly the failure mode we hit. The fix below appends the
//    matching `libclang_rt.builtins` archive to the link so `__clear_cache`
//    (and any other builtin libcall) is satisfied statically.
//
// `libm.a` is shipped by NDK r26 so it does not need a stub.
//
// On non-Android targets this build script is a no-op.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("-linux-android") {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let stub_dir = out_dir.join("android-link-stubs");
    fs::create_dir_all(&stub_dir).expect("create android-link-stubs dir");

    // (1) Empty `libpthread.a` so the upstream `-lpthread` is satisfied;
    // pthread symbols themselves resolve against bionic at load time.
    let empty_archive: &[u8] = b"!<arch>\n";
    let pthread_path = stub_dir.join("libpthread.a");
    fs::write(&pthread_path, empty_archive).expect("write libpthread.a stub");
    println!("cargo:rustc-link-search=native={}", stub_dir.display());

    // (2) Link `libclang_rt.builtins-<arch>-android.a` so `__clear_cache`
    // (called from QEMU's TCG cache-flush after codegen) and other compiler
    // builtins are resolved statically. Without this the .so ships with
    // `__clear_cache` as an undefined dynamic symbol that bionic does not
    // export, and the app crashes on `System.loadLibrary`.
    if let Some(builtins) = find_clang_rt_builtins(&target) {
        // Pass the archive directly to the link command. Using a full path
        // (instead of `-l<name>` plus `-L<dir>`) avoids any clash with
        // search-order quirks in `cargo-ndk`'s linker wrapper.
        println!("cargo:rustc-link-arg={}", builtins.display());
    } else {
        panic!(
            "pocket-android-jni: could not locate \
             libclang_rt.builtins for target {target}; \
             set ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) to your NDK root. \
             Android JIT (unicorn) will fail to dlopen without this archive."
        );
    }
}

/// Locate `libclang_rt.builtins-<arch>-android.a` inside the configured NDK,
/// matching the clang version that the NDK toolchain ships.
fn find_clang_rt_builtins(target: &str) -> Option<PathBuf> {
    let ndk_root = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("ANDROID_NDK_ROOT"))
        .map(PathBuf::from)?;

    let arch = clang_rt_arch_for_target(target)?;
    let host_tag = ndk_host_tag();

    let lib_clang = ndk_root
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(host_tag)
        .join("lib")
        .join("clang");

    // Pick the highest-numbered clang version directory that contains the
    // expected archive — NDK r26d ships clang 17, future NDKs may bump it.
    let mut best: Option<PathBuf> = None;
    let mut best_version: i64 = -1;
    if let Ok(entries) = fs::read_dir(&lib_clang) {
        for entry in entries.flatten() {
            let path = entry.path();
            let candidate = path
                .join("lib")
                .join("linux")
                .join(format!("libclang_rt.builtins-{arch}-android.a"));
            if candidate.is_file() {
                let version = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(parse_version)
                    .unwrap_or(-1);
                if version > best_version {
                    best_version = version;
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn clang_rt_arch_for_target(target: &str) -> Option<&'static str> {
    Some(match target {
        "aarch64-linux-android" => "aarch64",
        "armv7-linux-androideabi" => "arm",
        "i686-linux-android" => "i686",
        "x86_64-linux-android" => "x86_64",
        "riscv64-linux-android" => "riscv64",
        _ => return None,
    })
}

/// Best-effort host-tag for the NDK prebuilt directory. The CI runner and
/// developer machines targeted here all use `linux-x86_64`; this still
/// produces sensible names for other dev hosts that may clone the repo.
fn ndk_host_tag() -> String {
    match env::consts::OS {
        "macos" => "darwin-x86_64".into(),
        "linux" => "linux-x86_64".into(),
        "windows" => "windows-x86_64".into(),
        other => panic!("Unsupported NDK build host: {other}"),
    }
}

fn parse_version(name: &str) -> i64 {
    // Accept either bare numbers like "17" or "17.0.2"; sort by the first
    // numeric component so newer NDKs win.
    name.split('.')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_mapping_is_exhaustive_for_supported_abis() {
        assert_eq!(
            clang_rt_arch_for_target("aarch64-linux-android"),
            Some("aarch64")
        );
        assert_eq!(
            clang_rt_arch_for_target("armv7-linux-androideabi"),
            Some("arm")
        );
        assert_eq!(
            clang_rt_arch_for_target("x86_64-linux-android"),
            Some("x86_64")
        );
        assert_eq!(clang_rt_arch_for_target("i686-linux-android"), Some("i686"));
        assert_eq!(
            clang_rt_arch_for_target("riscv64-linux-android"),
            Some("riscv64")
        );
        assert_eq!(clang_rt_arch_for_target("x86_64-pc-windows-msvc"), None);
    }

    #[test]
    fn parse_version_picks_first_component() {
        assert_eq!(parse_version("17"), 17);
        assert_eq!(parse_version("17.0.2"), 17);
        assert_eq!(parse_version("18"), 18);
        assert_eq!(parse_version(""), -1);
        assert_eq!(parse_version("not-a-version"), -1);
    }
}
