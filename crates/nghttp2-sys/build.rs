use std::path::PathBuf;
use std::process::Command;

/// Cargo.toml から外部依存関係のメタデータを読み込む
fn load_external_dependency(name: &str) -> (String, String) {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let cargo_toml = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let toml = shiguredo_toml::from_str(&cargo_toml).unwrap();

    let deps = toml["package"]["metadata"]["external-dependencies"]
        .as_table()
        .expect("external-dependencies not found");

    let dep = deps[name].as_table().expect("dependency not found");
    let git = dep["git"].as_str().expect("git not found").to_string();
    let version = dep["version"]
        .as_str()
        .expect("version not found")
        .to_string();

    (git, version)
}

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Cargo.toml からメタデータを読み込む
    let (git_url, version) = load_external_dependency("nghttp2");

    // nghttp2 をクローン
    let nghttp2_dir = out_dir.join("nghttp2");
    if !nghttp2_dir.exists() {
        let tag = format!("v{}", version);
        let status = Command::new("git")
            .args(["clone", "--branch", &tag, "--depth", "1", &git_url])
            .arg(&nghttp2_dir)
            .status()
            .expect("Failed to execute git clone");
        if !status.success() {
            panic!("Failed to clone nghttp2 (tag: {})", tag);
        }
    }

    // nghttp2 ビルド (静的ライブラリのみ)
    //
    // ENABLE_LIB_ONLY=ON / ENABLE_HTTP3=OFF でもオプショナル依存の find_package が
    // 走り、Homebrew 等のシステム側ライブラリを暗黙に拾ってしまうため、
    // CMAKE_DISABLE_FIND_PACKAGE_* で全て抑止する。
    let nghttp2_dst = cmake::Config::new(&nghttp2_dir)
        .define("BUILD_STATIC_LIBS", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("ENABLE_LIB_ONLY", "ON")
        .define("BUILD_TESTING", "OFF")
        .define("ENABLE_DOC", "OFF")
        .define("ENABLE_HTTP3", "OFF")
        .define("WITH_LIBXML2", "OFF")
        .define("WITH_JEMALLOC", "OFF")
        .define("WITH_MRUBY", "OFF")
        .define("WITH_NEVERBLEED", "OFF")
        .define("WITH_LIBBPF", "OFF")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_quictls", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_libressl", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libngtcp2_crypto_wolfssl", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libnghttp3", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libev", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libevent", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libcares", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Jansson", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Jemalloc", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Systemd", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libbrotlienc", "ON")
        .define("CMAKE_DISABLE_FIND_PACKAGE_Libbrotlidec", "ON")
        .build();

    // ライブラリパス
    let lib_dir = if nghttp2_dst.join("lib64").exists() {
        nghttp2_dst.join("lib64")
    } else {
        nghttp2_dst.join("lib")
    };

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=nghttp2");

    // 依存クレートに情報を渡す
    println!("cargo:include={}/include", nghttp2_dst.display());

    #[cfg(feature = "overwrite")]
    overwrite_bindgen(&out_dir);
}

#[cfg(feature = "overwrite")]
fn overwrite_bindgen(out_dir: &PathBuf) {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // ビルド後の include ディレクトリ (nghttp2ver.h が生成される場所)
    let nghttp2_installed_include = out_dir.join("include");
    // ソースの include ディレクトリ (nghttp2.h がある場所)
    let nghttp2_source_include = out_dir.join("nghttp2/lib/includes");

    bindgen::Builder::default()
        .header(manifest_dir.join("src/wrapper.h").to_str().unwrap())
        .clang_arg(format!("-I{}", nghttp2_installed_include.display()))
        .clang_arg(format!("-I{}", nghttp2_source_include.display()))
        .allowlist_function("nghttp2_.*")
        .allowlist_type("nghttp2_.*")
        .allowlist_var("NGHTTP2_.*")
        .generate()
        .expect("Failed to generate nghttp2 bindings")
        .write_to_file(manifest_dir.join("src/bindings.rs"))
        .expect("Failed to write nghttp2 bindings");
}
