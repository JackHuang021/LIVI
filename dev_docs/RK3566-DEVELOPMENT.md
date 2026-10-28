# LIVI RK3566 适配开发过程

本文记录 LIVI 在 RK3566 测试板上的适配过程：有线 CarPlay / Android Auto、显示参数、界面缩放，以及简体中文本地化和中文字体支持。命令均以仓库根目录为基准。

## 1. 目标与硬件平台

- 板卡：RK3566，系统为 Armbian（Debian 13 trixie）。
- 屏幕：6 英寸 DSI 面板，原生分辨率 1080x2160（竖屏），系统中旋转为 2160x1080 横屏使用，像素密度约 360 DPI。
- 目标：
  - 使用离线 MFi 凭据跑通有线 CarPlay；
  - 支持有线 Android Auto；
  - 画面比例、UI 尺寸符合车机使用习惯；
  - 界面可切换为简体中文，手机推送的中文歌曲信息正常显示。

## 2. 运行结构与日志

LIVI 在板子上以 systemd 服务 `livi-kiosk.service` 运行，包含多个进程：

- `livi-core`：会话、USB、投影协议、设置与子进程管理；
- `LIVI`（`livi-ui`）：Slint 界面，由 core 启动；
- `livi-gst-host`：GStreamer 视频管线；
- `livi-compositor`：LIVI 自带 Wayland 合成器，直接驱动 DRM/KMS；
- `livi-helperd`：以 root 运行，处理蓝牙、Wi-Fi、接入点等特权操作。

日志位于 `~/.config/LIVI/log/`：

- `LIVI.log`：core 与 UI 的 stdout/stderr；
- `compositor.log`：合成器与 DRM 输出；
- `gst-host.log`：视频管线。

UI 会打印当前窗口尺寸和缩放比例，例如：

```
[ui] window 2160x1080 at scale 2
```

### 重启注意事项

在这个板子上不能直接 `systemctl restart livi-kiosk`：`ExecStopPost` 会启动 `getty@tty1`，与同一个 unit 的 start job 冲突，导致服务停在 inactive。正确做法是先 `stop` 再 `start`。

```sh
sudo systemctl stop livi-kiosk
sudo systemctl start livi-kiosk
```

## 3. 构建与测试环境

### 3.1 仓库的标准交叉构建

`scripts/linux-arm64/build.sh` 在 x86_64 主机上用 Podman + Debian Trixie 容器交叉编译 arm64，并生成 AppImage：

```sh
scripts/linux-arm64/build.sh /path/to/repo-copy [cache-dir]
```

容器内执行 `cargo xtask package-linux --arch arm64`，产物在 `dist/LIVI-<version>-linux-arm64.AppImage`。

### 3.2 在 arm64 Linux 主机上原生构建

如果开发机本身就是 arm64 Linux，可以不使用容器，直接安装 `scripts/linux-arm64/Containerfile` 中列出的构建依赖，然后：

```sh
cd native
cargo xtask package-linux --arch arm64
```

需要额外准备 aarch64 版 `appimagetool`，通过 `APPIMAGETOOL` 环境变量指定；该工具本身是 AppImage，无 FUSE 时可加 `APPIMAGE_EXTRACT_AND_RUN=1`。

构建产物 `dist/`、`out/` 已在 `.gitignore` 中，不应提交。

### 3.3 常用测试

```sh
cd native
cargo test -p livi-ui-model      # 设置模型、i18n、界面数据
cargo test -p livi-core nav_text # 导航提示词
```

如果访问 crates.io 较慢，可以只在本地 Cargo 配置中临时加镜像；构建完成后不要把镜像配置提交到仓库。

## 4. 离线 MFi 认证

实现说明见 `docs/offline-mfi.md`。测试板把 Apple 签发的软件身份放在：

```
~/.config/LIVI/mfi/certificate.p7b
~/.config/LIVI/mfi/identity.pk8
```

目录权限 0700、文件 0600。`livi-helperd` 启动时会打印：

```
MFi from the imported files in ...
```

然后启动有线/无线 CarPlay 的 watcher。

注意：真实 MFi 凭据不能提交到仓库。`.gitignore` 已忽略 `*.p7b`、`*.pk8` 和 `offline-mfi/`，仅放行测试用的 `native/crates/iap2-mfi/src/testdata/*`。文档和日志中也只记录路径，不记录凭据内容。

## 5. 有线 CarPlay / Android Auto 联通性

### 5.1 NCM 链路需要 IPv6

Armbian 默认设置 `net.ipv6.conf.default.disable_ipv6=1`。有线 CarPlay 通过 USB NCM 网卡与手机通信，需要 fe80 链路本地地址来告知手机 AirPlay 服务的位置，因此 NCM 接口必须单独打开 IPv6，否则 CarPlay bring-up 停在：

```
no link-local on "cpusb0"
```

修复位于 `native/crates/iap2-usbmux/src/ncm.rs`：接口启动前写入 `disable_ipv6=0`、`addr_gen_mode=0`，并允许 DAD。对应提交 `d27534b6`。

### 5.2 Android Auto 手机识别

部分 OPPO / OnePlus 手机的 USB 描述与原有识别表不匹配，已补充 USB 识别规则，对应提交 `3c7daadf`。

## 6. 显示适配

### 6.1 插上 iPhone 黑屏

现象：手机已连接、协议已建立，但屏幕全黑。

原因：RK3568 的 H.264 硬件解码器（`rockchip,rk3568-vpu-dec`）宽度上限为 1920 像素，V4L2 会把 2160 宽协商为 1920，而 `v4l2slh264dec` 不会自动缩放，导致协商失败。

解决：让 CarPlay 按 1920x960 推流，由合成器放大到 2160x1080：

```json
"projectionWidth": 1920,
"projectionHeight": 960
```

### 6.2 画面横向拉伸

现象：CarPlay 画面比例不对，图标看起来被拉宽。

原因：面板实际有效区域约 68x136 mm，但 DSI 驱动通过 DRM connector 上报的是 135x216 mm（按 10.1 英寸面板填写）。CarPlay 依据上报的物理尺寸判断像素是否方形，于是横向拉伸。

解决：设置 `projectionDpi`，使上报的物理尺寸变成与 1920x960 一致的 2:1：

```json
"projectionDpi": 220
```

### 6.3 LIVI 主界面图标和文字过小

现象：CarPlay 显示正常，但 LIVI 自己的主页图标、文字只有约 2 mm 高。

原因：UI 的缩放基准来自 Wayland 输出 scale，而 DRM 后端固定上报 scale 为 1（compositor 中 `OutputModeSource::Static` 和 `ensure_server_output` 都不提供实际 scale）。因此 2160x1080 的面板按 1x 逻辑像素渲染 1280x720 设计的界面。

解决：使用现有的 UI Zoom 设置，把 `uiZoomPercent` 从 100 调到 200（schema 允许的最大值）：

```json
"uiZoomPercent": 200
```

此时窗口为 2160x1080 物理像素、scale 2（1080x540 逻辑像素），图标约 4 mm、导航文字约 3 mm，接近常见 7 英寸 1280x720 车机的观感。

UI 缩放不影响 CarPlay/Android Auto 视频：视频是合成器单独放置的 surface，投影触摸坐标也在宽高比一致的矩形内归一化。

## 7. 简体中文支持

### 7.1 现状分析

- i18n 框架已存在：`livi-ui-model/src/i18n.rs` 从 `locales/*.json` 加载扁平化键值，缺键回退英文。
- 已有语言：`en`、`de`、`fr`、`ua`；没有中文选项。
- 字体：UI 只内嵌了 Roboto 的四个字重子集，不含任何 CJK 字形；测试板系统也没有安装中文字体（`fc-list :lang=zh` 为空）。
- 因此中文界面既无法选择，手机推送的中文歌曲名也会显示为方框（tofu）。

### 7.2 中文语言包与语言选项

- 新增 `native/crates/livi-ui-model/locales/zh.json`，覆盖设置、播放、DAB、GPS、软件更新、按键绑定等全部 331 个界面文案。
- `i18n.rs` 注册 `ZH`，并把 `zh`、`zh-CN`、`zh-Hans`、`cn` 映射到简体中文。
- 设置 schema 的语言选项新增 `settings.chinese` / `zh`，位于“设置 > 通用 > 语言”。
- 语言切换在运行时生效：配置变化后 `apply_look()` 更新 `Strings`，设置页重新渲染，播放页的“手机 / DAB+”等文本也同步刷新。

### 7.3 清理写死的英文

以下文本原来直接写在 Slint 或 Rust 中，不随语言变化，已改为可翻译文案：

| 位置           | 原文                                     | 中文                      |
| -------------- | ---------------------------------------- | ------------------------- |
| 播放页无封面   | `No Artwork`                             | 暂无封面                  |
| 摄像头页无设备 | `No camera found`                        | 未找到摄像头              |
| 软件更新阶段   | `Downloading` / `Ready to install` / ... | 正在下载 / 准备安装 / ... |
| 仪表盘挡位标题 | `GEAR`                                   | 挡位                      |
| 副屏无内容     | `No content on this screen`              | 此屏幕无内容              |
| 仪表盘无数据   | `No dashboard on this screen`            | 此屏幕无仪表盘            |

### 7.4 导航提示词

`native/crates/livi-core/src/nav_text.rs` 增加了 `Language::Zh` 和完整的中文 maneuver 词典（左转、右转、环岛出口、驶出高速、变更高速等），并补充了未知路况、无路线、环岛出口格式的中文。这样仪表盘和 CarPlay 导航卡片中的转向提示也会跟随语言设置。

### 7.5 中文字体

Roboto 只覆盖拉丁字符，测试板也没有系统 CJK 字体，因此必须随 AppImage 提供一个中文字体：

- 字体：Noto Sans SC（OFL-1.1），从可变字重版本中裁剪子集；
- 覆盖范围：GB2312 常用简体 + Big5 一级常用繁体，另含拉丁、标点、全角符号；
- 产物：`native/crates/livi-ui/ui/fonts/NotoSansSC-LIVI.ttf`，约 5.5 MB；
- 打包：`xtask package-linux` 把字体复制到 AppImage 的 `resources/fonts/`；
- 启动：AppImage 的启动脚本导出
  `SLINT_FONT_PATH="$HERE/resources/fonts${SLINT_FONT_PATH:+:$SLINT_FONT_PATH}"`。

Slint 启动时会把 `SLINT_FONT_PATH` 中的字体注册进通用 SansSerif 回退链，因此文本渲染顺序为：

```
Roboto（拉丁） -> 通用 SansSerif -> Noto Sans SC 子集（中文） -> 系统 fontconfig 回退
```

这样英文界面仍然是 Roboto，中文标签、歌名、歌手、专辑名等手机元数据自动回退到 Noto Sans SC；不需要在板子上安装系统字体。`CREDITS.md` 已补充 Noto Sans SC 的许可说明。

### 7.6 验证方式

**主机侧无头渲染**

在 Linux 主机上可以用 Weston headless 加 UI fixture 直接渲染中文页面并截图：

```sh
weston --backend=headless-backend.so --renderer=gl \
  --socket=wayland-livi --width=1280 --height=720 --idle-time=0

WAYLAND_DISPLAY=wayland-livi \
LIVI_UI_STATE=/tmp/livi-zh-system.json \
LIVI_UI_PAGE=settings LIVI_UI_SETTINGS=general \
LIVI_UI_SNAPSHOT=/tmp/livi-zh-settings.png \
SLINT_FONT_PATH=native/crates/livi-ui/ui/fonts/NotoSansSC-LIVI.ttf \
./native/target/release/LIVI
```

`LIVI_UI_STATE` 使用 `native/crates/livi-ui/fixtures/*.json` 中的状态，可把 `config.language` 改为 `zh`、把 `nowPlaying` 改成中文歌曲来验证中文渲染。

**板子侧**

- 重新打包 AppImage 并安装到 `~/LIVI/LIVI.AppImage`；
- 把 `~/.config/LIVI/config.json` 的 `language` 改为 `zh`；
- 重新 `stop` / `start` kiosk；
- 日志确认 `[ui] window 2160x1080 at scale 2`；
- 通过 `LIVI_UI_SNAPSHOT` 诊断开关导出 UI 截图，确认设置页中文标签无方框；
- 手机通过 CarPlay 播放中文歌曲，确认歌名、歌手、专辑正常显示（实测使用网易云音乐中文元数据）。

**自动化测试**

- `cargo test -p livi-ui-model`：90 项通过，其中包含中文语言包完整性和“所有设置标签都有翻译”的检查；
- `cargo test -p livi-core nav_text`：3 项通过，包含中文转向词条断言。

## 8. 问题与解决汇总

| 现象                                          | 根因                                | 解决                                                |
| --------------------------------------------- | ----------------------------------- | --------------------------------------------------- |
| 插上 iPhone 后黑屏                            | VPU 解码宽度上限 1920，解码器不缩放 | 投影改为 1920x960，由合成器放大                     |
| CarPlay 画面横向拉伸                          | 面板驱动上报的物理尺寸错误          | `projectionDpi: 220`，上报 2:1 尺寸                 |
| LIVI 界面图标/文字过小                        | DRM 后端总是上报 Wayland scale 1    | `uiZoomPercent: 200`                                |
| 中文界面无法选择                              | 语言选项只有 en/de/fr/ua            | 新增 zh 语言包和设置项                              |
| 中文歌曲名显示为方框                          | Roboto 无 CJK 字形，板子无中文字体  | AppImage 内置 Noto Sans SC 子集 + `SLINT_FONT_PATH` |
| 有线 CarPlay 停在 `no link-local on "cpusb0"` | NCM 接口未启用 IPv6                 | NCM 启动前逐接口打开 IPv6                           |
| 部分 OPPO/OnePlus 手机 AA 不识别              | USB 识别表不完整                    | 补充 USB 识别规则                                   |

## 9. 板子配置与备份

测试板 `~/.config/LIVI/config.json` 的关键配置：

| 键                 | 值     | 说明                    |
| ------------------ | ------ | ----------------------- |
| `language`         | `zh`   | 简体中文界面            |
| `uiZoomPercent`    | `200`  | 高 DPI 面板下的 UI 缩放 |
| `projectionWidth`  | `1920` | 避开 VPU 宽度限制       |
| `projectionHeight` | `960`  | 与 1920 组成 2:1        |
| `projectionDpi`    | `220`  | 修正上报的物理尺寸      |
| `mainScreenWidth`  | `2160` | 主屏宽度                |
| `mainScreenHeight` | `1080` | 主屏高度                |

升级前保留了以下备份（只在测试板上，不在仓库中）：

- AppImage：`~/LIVI/LIVI.AppImage.old-20261026-1556`
- 配置：`~/.config/LIVI/config.json.bak-20261026-1556`

## 10. 相关提交

2026-10-28 已 rebase 到 `origin/main`（`3e2a54eb`）。主线同期包含 crate 扁平化、GStreamer 辅助 crate 合并、zbus 6、SPDX/dprint 与法务信息、UI 主题色等更新；下表为 rebase 后的提交号。

| 提交       | 内容                                                                    |
| ---------- | ----------------------------------------------------------------------- |
| `d27534b6` | carplay: enable IPv6 on the USB NCM bridge                              |
| `6e589a1d` | mfi: verify offline credentials and digest lengths                      |
| `3c7daadf` | android-auto: usb: recognize OPPO and OnePlus phones                    |
| `72d3d8ac` | fix(install): skip i2c buses claimed in the device tree for MFi（主线） |
| `8f97e6db` | fix(hfp): wait for the SCO link before reading call audio（主线）       |
| `c7c6fc07` | core: add Chinese navigation instructions                               |
| `18b0d80f` | ui: add Simplified Chinese translations                                 |
| `a9a1227b` | ui: bundle a CJK fallback font for Chinese text                         |

## 11. 已知限制与后续工作

- 中文字体子集覆盖 GB2312 和 Big5 一级字。非常用字、生僻字或二级/三级繁体字仍可能缺字；如果系统安装了完整 CJK 字体，会继续走 fontconfig 回退。
- UI Zoom 的上限仍是 schema 的 200%。如果以后更换更高 DPI 或更小尺寸的面板，需要提高这个上限，或让 UI 根据面板物理尺寸自动选择基准 scale。
- 无线 CarPlay / 无线 Android Auto 不在本次适配范围内，仍使用原有的无线链路实现。
- 触摸、音频（音乐、电话、麦克风）和方向盘按键已具备实现，建议在整车上做一轮完整验收。
- AppImage 更新会保留旧的用户配置；语言、UI Zoom、投影参数位于 `config.json`，升级后应确认这些值未被覆盖。

## 12. 主要文件索引

| 路径                                                          | 作用                            |
| ------------------------------------------------------------- | ------------------------------- |
| `native/crates/livi-ui-model/src/i18n.rs`                     | 语言包注册与翻译查找            |
| `native/crates/livi-ui-model/locales/zh.json`                 | 简体中文文案                    |
| `native/crates/livi-ui-model/src/settings/schemas/general.rs` | 语言选项、UI Zoom 等设置项      |
| `native/crates/livi-ui/src/main.rs`                           | 语言切换、UI 缩放、界面状态更新 |
| `native/crates/livi-ui/src/media_ui.rs`                       | 播放页文案                      |
| `native/crates/livi-ui/src/secondary.rs`                      | 副屏窗口文案                    |
| `native/crates/livi-ui/ui/app.slint`                          | 主窗口与摄像头页                |
| `native/crates/livi-ui/ui/dashboard.slint`                    | 仪表盘                          |
| `native/crates/livi-ui/ui/dashboard/state.slint`              | 仪表盘状态与文案属性            |
| `native/crates/livi-ui/ui/fonts/NotoSansSC-LIVI.ttf`          | 内置中文回退字体子集            |
| `native/crates/livi-core/src/nav_text.rs`                     | 导航提示词（含中文）            |
| `native/xtask/src/package_linux.rs`                           | AppImage 打包与字体部署         |
| `scripts/linux-arm64/build.sh`                                | arm64 交叉构建入口              |
| `docs/offline-mfi.md`                                         | 离线 MFi 认证实现说明           |
