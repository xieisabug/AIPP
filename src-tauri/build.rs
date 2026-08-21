fn main() {
    if let Ok(protoc_path) = protoc_bin_vendored::protoc_bin_path() {
        std::env::set_var("PROTOC", protoc_path);
    }
    // 注意：build.rs 编译并运行在主机上，`#[cfg(windows)]` 反映的是主机平台而不是
    // 目标平台。Windows 主机上交叉编译 Android 时主机 cfg 仍为 windows，会把
    // embed-resource 产出的 Windows COFF .lib 传给 Android 链接器，导致
    // "unknown file type" 链接失败。这里必须按目标平台（CARGO_CFG_TARGET_OS）判断。
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        // tauri-build 默认把应用 manifest（含 common-controls v6 依赖）只链接进 bin
        // 目标，lib 的测试二进制没有 manifest，加载时 TaskDialogIndirect 解析失败
        // （0xc0000139）。这里改为：不让 tauri-build 嵌 manifest，统一用
        // embed-resource 把同等 manifest 链接进所有目标（bin/cdylib/test）。
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("tauri build script failed");
        embed_app_manifest_for_all_targets();
    } else {
        tauri_build::build();
    }
}

// 不按主机平台 cfg 掉这个函数：它和上面的 target_os 检查保持一致，在所有主机上都可编译，
// 仅在目标平台是 Windows 时才真正执行（embed-resource 支持从任意主机为 Windows 目标嵌入资源）。
fn embed_app_manifest_for_all_targets() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set in build scripts");
    let manifest_path = std::path::Path::new(&out_dir).join("aipp-app.manifest");
    // 内容与 tauri-build 的默认 windows-app-manifest.xml 保持一致
    std::fs::write(
        &manifest_path,
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
          type="win32"
          name="Microsoft.Windows.Common-Controls"
          version="6.0.0.0"
          processorArchitecture="*"
          publicKeyToken="6595b64144ccf1df"
          language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#,
    )
    .expect("write app manifest");
    let rc_path = std::path::Path::new(&out_dir).join("aipp-app-manifest.rc");
    std::fs::write(
        &rc_path,
        format!("1 24 \"{}\"\r\n", manifest_path.display().to_string().replace('\\', "\\\\")),
    )
    .expect("write app manifest rc");
    embed_resource::compile_for_everything(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("compile app manifest resource");
}
