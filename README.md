# 灵动岛

一个挂在 Mac 刘海上的轻量灵动岛。它会读取内建屏幕的刘海安全区，窗口贴住屏幕
物理顶边并从刘海向下展开。通过系统的 `NSWorkspace` 读取当前聚焦的 App，实时显示
App 名称、原生图标和本次专注时长；交互不会抢走当前 App 的焦点。

## 本地运行

```bash
npm install
npm run app:dev
```

## 更新方式

- **App 自动更新**：发布 `v*` 标签后，GitHub Actions 构建 DMG、创建 Release，并更新
  `Casks/lingdongdao.rb`。App 内会比较当前版本和 Homebrew cask 最新版本，确认后通过
  Homebrew 升级并重启。
- **界面热更新**：推送 `src/**` 到 `main` 后，GitHub Actions 会把自包含 HTML 和
  SHA-256 清单发布到滚动 `webui` Release。已安装 App 每 15 秒检查一次，校验通过后
  原子替换并刷新；不兼容或校验失败会继续使用内置界面。

首次发布前，需要把代码推送到 `Gloomysunday28/message-kit`。发布后可使用：

```bash
brew tap Gloomysunday28/message-kit https://github.com/Gloomysunday28/message-kit
brew install --cask lingdongdao
```

版本号需同步修改：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src/webui-compat.json`（只在原生接口不兼容时提高最低版本）
