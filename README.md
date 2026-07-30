# FuckTheBug

基于 Tauri 2、Vue 3、TypeScript 和 SCSS 的 JIRA 桌面问题单追踪器。

## 功能

- 使用 JIRA 9 REST API 查询当前账号的问题单
- 支持多个可配置 JQL 视图和独立计数
- Token 保存到系统凭据管理器，不写入源码或 JSON 配置
- 悬浮计数器、问题单详情、手动刷新和外部跳转
- 新问题单红灯提醒和安装版Windows原生通知
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

## 打包

```bash
npm run tauri:build
```

配置保存在 Tauri 的用户配置目录。JIRA Token 仅保存在系统凭据管理器中，服务名为 `com.genata.bug-ticker`。
