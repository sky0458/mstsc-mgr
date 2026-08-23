# mstsc-mgr-external

`mstsc-mgr-external` 是 `mstsc-mgr` 的独立 legacy 产品线，专门面向 **Windows Server 2016 / Windows 10 1607+ x64**。

它只做一件事：保存 MSTSC 连接账号，并通过系统 Credential Manager 自动填充凭据后启动外部 `mstsc.exe`。

## 特性

- 纯 Rust + 原生 Win32 控件。
- 不使用 GPUI、WebView、Electron、.NET/WPF/WinUI、DirectX UI。
- 保存连接名称、IP/主机名、用户名和密码。
- 密码使用当前 Windows 用户的 DPAPI 加密后写入：
  `%LOCALAPPDATA%\mstsc-mgr-external\connections.json`
- 连接时调用 `CredWriteW` 写入 `TERMSRV/<host>` 凭据，然后启动：
  `mstsc.exe /v:<host>`
- 用户名和密码不会放入 `mstsc.exe` 命令行。
- 删除连接时同步尝试删除对应的 Windows Credential Manager 凭据。
- 编辑已有连接时，密码留空表示保留原密码。

## 使用方式

1. 启动 `mstsc-mgr-external.exe`。
2. 点击“新建”，填写名称、IP/主机名、用户名和密码。
3. 点击“保存”。
4. 在左侧选中连接，点击“连接”，或双击连接条目。
5. 程序会写入 `TERMSRV/<host>` 系统凭据并启动外部 MSTSC。

## 安全说明

配置文件中不保存明文密码。`password_dpapi` 是 Windows DPAPI 当前用户范围密文，因此配置文件默认只能由同一 Windows 用户配置文件解密。

## 构建

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Release 构建静态链接 MSVC CRT，以降低 Windows Server 2016 上缺少 VC++ Runtime 的部署依赖。

## Development Log

### version 0.1.0 2026-08-23 20:57:00

- 新建完全独立的 `external/server2016` Rust 产品线。
- 使用原生 Win32 控件实现连接账号列表、新建、编辑、删除和一键连接。
- 使用 DPAPI 加密本地密码，使用 `CredWriteW` 写入 MSTSC 的 `TERMSRV/<host>` 凭据。
- 明确移除 GPUI、DirectX、WebView、浮球、托盘、热键、会话扫描、KeepAlive、导入导出等主线功能。
- CI 针对 Windows x64 构建，并检查最终 EXE 不依赖 DirectX、ICU 或 WebView 运行库。
- 增加隔离分支的 pull-request CI 验证路径；该 PR 仅用于构建验证，不合并回主线。
- 按 rustfmt 整理初始 Win32 源码，并移除未使用的 UI 常量。
- 按 `windows-rs 0.61` 的实际 Win32 API 签名修正 HMENU、CredDeleteW 和 SendMessageW 调用。

## License

MIT
