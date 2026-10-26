use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/fulgurstack.ico");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSDKVersion");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("fulgurstack.res");
    let status = Command::new(resource_compiler())
        .current_dir(root.join("assets"))
        .arg("/nologo")
        .arg("/fo")
        .arg(&output)
        .arg("windows.rc")
        .status()
        .expect("Windows SDK resource compiler is required to embed the application icon");
    assert!(
        status.success(),
        "Application icon resource compilation failed"
    );
    println!(
        "cargo:rustc-link-arg-bin=fulgurstack-gui={}",
        output.display()
    );
}

fn resource_compiler() -> PathBuf {
    if let Some(path) = env::var_os("RC") {
        return path.into();
    }
    let host = env::var("HOST").unwrap_or_default();
    let arch = if host.starts_with("aarch64") {
        "arm64"
    } else {
        "x64"
    };
    if let (Some(sdk), Ok(version)) = (env::var_os("WindowsSdkDir"), env::var("WindowsSDKVersion"))
    {
        let path = PathBuf::from(sdk)
            .join("bin")
            .join(version.trim_end_matches(['\\', '/']))
            .join(arch)
            .join("rc.exe");
        if path.is_file() {
            return path;
        }
    }
    if let Some(program_files) = env::var_os("ProgramFiles(x86)") {
        let bin = PathBuf::from(program_files).join("Windows Kits/10/bin");
        let mut versions: Vec<_> = fs::read_dir(bin)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join(arch).join("rc.exe").is_file())
            .collect();
        versions.sort_by_key(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
        if let Some(version) = versions.pop() {
            return version.join(arch).join("rc.exe");
        }
    }
    "rc.exe".into()
}
