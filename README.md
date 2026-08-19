# FuckTheBug

基于 Tauri 2、Vue 3、TypeScript 和 SCSS 的 JIRA 桌面问题单追踪器。

## 功能

- 使用 JIRA 9 REST API 查询当前账号的问题单
- 支持多个可配置 JQL 视图和独立计数
- Token 保存到系统凭据管理器，不写入源码或 JSON 配置
- 悬浮计数器、问题单详情、手动刷新和外部跳转
- 新问题单红灯提醒和安装版Windows原生通知
- 设置页显示当前版本，支持签名在线更新和自动重启
- 系统托盘显示、隐藏、设置和退出
- 保存窗口位置，调整尺寸时限制在有效显示器内
- 阻止同一视图并发请求，并显示明确的认证和网络错误

## 环境

- Node.js 24+
- Rust 1.95+
- Windows：Visual Studio 2022 Build Tools，包含 MSVC 和 Windows SDK

## 开发

```bash
npm install
npm run tauri:dev
```

开发构建只显示新问题单红灯，不发送Windows通知。安装版通过已注册的应用标识发送通知，避免被系统归类为PowerShell。

Windows 普通终端如果没有加载 MSVC 环境，可先执行：

```bat
call D:\VisualStudioBuildTools\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64
```

## 检查

```bash
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

## 发布

本项目不在开发电脑本地打包。发布前同步修改 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号，然后推送对应标签：

```bash
git tag v2.1.4
git push origin v2.1.4
```

`v*` 标签会触发 GitHub Actions，在 Windows Runner 中构建 NSIS 安装包、签名更新包并创建 GitHub Release。Action 随后更新 `updater/latest.json` 中的稳定下载地址，应用通过设置页的“检查更新”读取该清单，验签通过后安装并自动重启。

更新签名私钥和密码仅保存在仓库的 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` Secrets 中，禁止写入源码。

配置保存在 Tauri 的用户配置目录。JIRA Token 仅保存在系统凭据管理器中，服务名为 `com.genata.bug-ticker`。
