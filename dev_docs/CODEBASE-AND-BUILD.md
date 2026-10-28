# LIVI 代码分析与构建流程

> 分析对象：本仓库 `rk3566_dev` 分支（2026-10-26，版本 `10.1.0`；2026-10-28 已 rebase 到 `origin/main`）
> 分析日期：2026-10-26
> 配套文档：[README.md](README.md)（功能与安装）、[DEBUGGING.md](DEBUGGING.md)（调试开关）、[LIVI-LINK.md](LIVI-LINK.md)（无线适配器）、[RK3566-DEVELOPMENT.md](RK3566-DEVELOPMENT.md)（本板适配记录）

本文分三部分：**代码结构**（第 1–7 章）、**构建与发布流程**（第 8 章）、**测试、安全与二次开发**（第 9–12 章）。所有结论都对应到具体文件，便于按图索骥。

---

## 目录

1. [项目概览](#1-项目概览)
2. [仓库结构与代码规模](#2-仓库结构与代码规模)
3. [运行时架构](#3-运行时架构)
4. [投影协议栈](#4-投影协议栈)
5. [媒体管线](#5-媒体管线)
6. [UI 层](#6-ui-层)
7. [配置、状态与持久化](#7-配置状态与持久化)
8. [构建与发布流程](#8-构建与发布流程)
9. [测试、调试与质量保障](#9-测试调试与质量保障)
10. [安全模型](#10-安全模型)
11. [二次开发指南](#11-二次开发指南)
12. [已知限制与技术债](#12-已知限制与技术债)

- [附录 A：关键常量速查](#附录-a关键常量速查)
- [附录 B：核心文件索引](#附录-b核心文件索引)
- [附录 C：术语表](#附录-c术语表)

---

## 1. 项目概览

### 1.1 它是什么

LIVI（Linux In-Vehicle Infotainment）是一个开源的 **Apple CarPlay / Android Auto 车机主机**，用 Rust 编写，支持 Linux（x86_64、arm64，含树莓派等嵌入式板）与 macOS。它不依赖手机投屏盒子或厂商 SDK，而是**在配件侧原生实现** CarPlay 配件协议与 Android Auto 车机协议。

三个关键设计取向，贯穿整个代码库：

| 取向                            | 体现                                                                                         |
| ------------------------------- | -------------------------------------------------------------------------------------------- |
| **进程隔离**                    | `livi-core` 编排，UI、GStreamer、合成器、特权助手各跑独立进程，通过 Unix socket/控制协议通信 |
| **原生渲染，零拷贝**            | 手机下发的 H.264/H.265 码流直接进 GStreamer，解码后以 dma-buf 交给自研 Wayland 合成器上屏    |
| **无特权主进程 + 最小特权助手** | 只有 `livi-helperd` 以 root 运行，且 sudoers 只放行这一个二进制                              |

### 1.2 功能面

| 类别         | 能力                                                                      | 主要实现位置                                                            |
| ------------ | ------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| CarPlay      | 有线 + 无线、主屏 + 仪表屏、音频/通话/麦克风/Siri、多设备、车辆数据、气候 | `livi-cp`、`iap2-*`、`livi-runtime`                                     |
| Android Auto | 有线 + 无线、主屏 + 仪表屏、音频/输入/传感器/导航                         | `livi-aa`、`livi-aa-stack`、`livi-aa-proto`                             |
| 媒体         | 视频解码、音频混音、可视化频谱、摄像头、DAB+ 收音机                       | `livi-gst-host`、`livi-video*`、`livi-audio`、`livi-dab`、`livi-rtlsdr` |
| 显示         | 多窗口（主屏/仪表/副屏）、直连 DRM/KMS、窗口模式、gamma/对比度/色彩校正   | `livi-compositor`                                                       |
| UI           | Slint 自绘界面、5 种语言、设置树、仪表盘、媒体页                          | `livi-ui`、`livi-ui-model`                                              |
| 车辆         | 遥测（HTTP/Socket.IO/CLI）、GPS/NMEA/UBX、时区与对时                      | `livi-core`（`telemetry.rs`、`gnss/`）                                  |
| 无线适配器   | LIVI Link：网络 MFi、Wi-Fi AP、蓝牙                                       | `livi-link-*`、`livid`                                                  |
| 运维         | 自更新、kiosk 开机自启、udev/sudoers 自安装                               | `livi-core`（`update.rs`、`kiosk_unit.rs`、`udev_rule.rs`）             |

### 1.3 关键外部约束

- **MFi**：原生 CarPlay 必须通过 Apple MFi 认证。LIVI 既不内置也不绕过认证芯片，支持板载 I²C 认证协处理器、网络认证（LIVI Link），以及本分支的**离线软件凭据**（`~/.config/LIVI/mfi/`）。
- **无线**：CarPlay 无线需要蓝牙 + Wi-Fi AP；Android Auto 无线实际要求 5 GHz（详见 4.3）。
- **GStreamer**：视频/音频管线全部由 GStreamer 承担；Linux 版 AppImage 自带一份打过补丁的 GStreamer（见 8.5）。
- **显示**：Linux 上要么由 `livi-compositor` 直接接管 DRM/KMS（headless/kiosk），要么以嵌套 Wayland 窗口运行在桌面会话里。

---

## 2. 仓库结构与代码规模

### 2.1 顶层目录

```
LIVI/
├── README.md / DEBUGGING.md / LIVI-LINK.md / RK3566-DEVELOPMENT.md
├── native/                  # 全部 Rust 代码（Cargo workspace）
│   ├── Cargo.toml           # workspace：44 个 crate + xtask，version 10.1.0
│   ├── .cargo/config.toml   # xtask 别名 + 两个交叉编译 target 的 linker/rustflags
│   ├── crates/              # 44 个 crate（见 2.2）
│   ├── xtask/               # 构建/打包驱动（build-native / package-linux / package-mac）
│   └── vendor/femtovg/      # 打过补丁的 femtovg（Slint 的 GL 渲染后端）
├── assets/
│   ├── gstreamer/           # 预编译 GStreamer 运行时（linux-x64/arm64、macos-arm64），约 538 MB
│   ├── linux/               # systemd/udev/sudoers 模板、metainfo、触摸过滤脚本
│   ├── displays/            # 车机屏 EDID（奔驰/沃尔沃）
│   ├── icons/               # 桌面图标
│   ├── livi-link/…          # LIVI Link 固件产物（由 CI 提交）
│   └── mac/                 # macOS entitlements / icns
├── scripts/
│   ├── install/             # 安装器（desktop/headless 两条流程 + common.sh）
│   ├── gstreamer/           # 构建带补丁的 GStreamer 运行时
│   ├── linux-arm64/         # Podman 交叉构建 arm64 AppImage
│   └── livi-link/           # 各 SoC 的固件构建（内核/DTS/initramfs/rootfs）
├── docs/                    # 项目主页 index.html 与截图
├── dist/                    # 本地打包产物（.gitignore）
└── out/bin/                 # xtask build-native 产物（.gitignore）
```

### 2.2 crate 全表（按职责分层）

> LOC 为各 crate `src/**/*.rs`（含测试）的行数，统计于本次分析。

**入口与编排**

| crate             |    LOC | 职责                                                                                |
| ----------------- | -----: | ----------------------------------------------------------------------------------- |
| `livi-core`       | 26,123 | 主守护进程：配置、状态中枢、会话管理、投影驱动、遥测、AP 跟随、更新、UI socket 服务 |
| `livi-helperd`    |  2,575 | root 助手：蓝牙、Wi-Fi AP、MFi/USB 设备接入、udev/kiosk 安装                        |
| `livi-runtime`    |  8,294 | 助手侧运行时：CarPlay/AA bring-up、共享 socket、蓝牙/Wi-Fi、异步 MFi、Bonjour       |
| `livi-ui`         |  3,859 | Slint UI 可执行文件 `LIVI`                                                          |
| `livi-ui-model`   |  7,670 | UI 模型：外壳、仪表盘、设置树、i18n、图标                                           |
| `livi-compositor` |  5,406 | 自研嵌套 Wayland 合成器（DRM/KMS 直接后端 + 宿主窗口后端）                          |
| `livi-gst-host`   |  3,962 | 独立 GStreamer 宿主进程：视频/音频/麦克风/摄像头/URL 播放                           |

**协议与传输**

| crate           |    LOC | 职责                                                                             |
| --------------- | -----: | -------------------------------------------------------------------------------- |
| `livi-cp`       | 20,760 | CarPlay 协议栈：配对、iAP2、RTSP、视频/音频流、控制加密、HID、车辆状态、HLS 代理 |
| `iap2-link`     |    955 | iAP2 链路层：帧格式、校验、会话 ID、重传                                         |
| `iap2-csm`      |  1,425 | iAP2 CSM（控制会话消息）编解码                                                   |
| `iap2-mfi`      |    972 | MFi 认证：I²C 协处理器 / 文件身份 / 网络认证                                     |
| `iap2-usbmux`   |  1,482 | USB 侧：iPhone 发现、NCM/NTB、usbmux 多路复用                                    |
| `iap2-wired`    |    554 | 有线 carkit 通道（经 usbmuxd）                                                   |
| `livi-aa`       |  2,198 | Android Auto 传输层：TLS、USB AOAP、TCP 服务                                     |
| `livi-aa-stack` | 10,068 | AA 会话栈：服务发现、通道复用、媒体、导航、传感器、输入                          |
| `livi-aa-proto` |    148 | AA protobuf 定义（prost 生成）                                                   |
| `livi-hfp`      |    471 | HFP 免提端（AG）状态机，用于无线 AA 的蓝牙触发                                   |
| `livi-bt-host`  |  3,686 | 自带蓝牙协议栈（HCI/L2CAP/RFCOMM/SDP），用于 macOS 与 LIVI Link                  |
| `livi-btd`      |    562 | LIVI Link 上的蓝牙守护进程（Linux HCI）                                          |

**媒体与显示**

| crate                |   LOC | 职责                                                 |
| -------------------- | ----: | ---------------------------------------------------- |
| `livi-media`         | 2,595 | 粘合层：GStreamer 宿主客户端、合成器控制、plane/裁剪 |
| `livi-video`         | 1,050 | 编解码器选择、NAL 处理、码流分发                     |
| `livi-video-player`  | 1,535 | GStreamer 视频播放器、摄像头、URL 播放               |
| `livi-video-view`    |   673 | macOS 侧视频视图                                     |
| `livi-audio`         | 1,446 | 音频播放/上行/流定义（RTP → PulseAudio）             |
| `livi-screen-stream` |   399 | ChaCha20-Poly1305 加密的屏幕码流接收器               |
| `livi-crypto`        |   113 | ChaCha20-Poly1305 封装（aws-lc-rs）                  |

**无线、网络与 LIVI Link**

| crate                     |   LOC | 职责                                                                 |
| ------------------------- | ----: | -------------------------------------------------------------------- |
| `livi-wifi`               | 2,127 | hostapd 配置生成、频段/信道、AP 控制                                 |
| `livi-wifid`              | 1,157 | LIVI Link 的 Wi-Fi AP 守护进程                                       |
| `livi-net`                |   272 | TCP 连接工具（IPv4 优先）                                            |
| `livi-mdns`               |   513 | mDNS 守护进程（LIVI Link 的 `livi-link.local`）                      |
| `livi-link-host`          | 1,074 | 主机侧 dongle 支持：USB 链路、AP、蓝牙隧道                           |
| `livi-link-provision`     | 3,578 | dongle 固件探测/刷写/救援库                                          |
| `livi-link-provision-cli` | 1,165 | 刷机 CLI                                                             |
| `livid`                   | 1,636 | dongle 上的多调用二进制（netd/httpd/wifid/ledd/btd/accessoryd/mfid） |
| `livi-accessoryd`         | 1,071 | dongle 侧 CarPlay 配件服务（SDP/iAP 服务记录）                       |
| `livi-web`                | 1,094 | dongle Web 管理界面与固件写入                                        |
| `imx6ul-uboot`            |   660 | i.MX6UL dongle 的 U-Boot 环境读写                                    |
| `mkbootimg-v821b`         |   118 | V821B 启动镜像打包                                                   |

**协议定义与基础设施**

| crate              |   LOC | 职责                                                |
| ------------------ | ----: | --------------------------------------------------- |
| `livi-core-proto`  | 2,814 | UI↔core 协议、状态模型、配置、主题、遥测、GNSS 定义 |
| `livi-core-client` |   177 | UI 侧的 core 客户端库                               |
| `livi-host-proto`  |   705 | core↔gst-host / compositor 的二进制协议             |
| `livi-session-io`  |   629 | 会话 socket、USB 流、行协议                         |
| `livi-dab`         | 2,972 | DAB+ 解调（RTL-SDR 采样 → 音频）                    |
| `livi-rtlsdr`      |   761 | RTL-SDR USB 驱动                                    |

### 2.3 技术栈

| 组件      | 版本/选型                                          | 说明                                              |
| --------- | -------------------------------------------------- | ------------------------------------------------- |
| Rust      | edition 2024，toolchain `beta-2026-10-03`          | `rust-toolchain.toml` 固定，含 clippy/rustfmt     |
| UI        | Slint 1.18.1（femtovg + software 渲染）            | `livi-ui`；femtovg 使用 `vendor/femtovg` 补丁版本 |
| 多媒体    | gstreamer-rs 0.25.4 / gstreamer-app 0.25.2         | 视频、音频、摄像头、URL 播放                      |
| 合成器    | smithay 0.7 + smithay-client-toolkit 0.21          | DRM/KMS、GBM、EGL、libinput、libseat              |
| 异步      | tokio 1.54.1                                       | 主运行时                                          |
| 蓝牙      | zbus 6.1（BlueZ D-Bus）+ 自带 HCI 栈               | Linux 用 BlueZ；macOS/dongle 用 `livi-bt-host`    |
| USB       | nusb 0.2.9                                         | iPhone/Android/dongle 的 USB 访问                 |
| HTTP/遥测 | axum 0.8.9 + socketioxide 0.18.7                   | `:4000` 遥测入口                                  |
| 加密      | aws-lc-rs（ChaCha20-Poly1305）、OpenSSL（CarPlay） | 见 10.4                                           |
| 序列化    | serde/serde_json、prost 0.14                       | UI 协议用 JSON；AA 用 protobuf                    |
| 打包      | appimagetool 1.9.0、codesign/dmg                   | Linux AppImage、macOS DMG                         |

### 2.4 代码组织与风格

- 每个 crate 一个目录，`src/lib.rs` 为主，二进制入口在 `src/main.rs`。
- 单元测试内联在模块底部 `#[cfg(test)] mod tests`；协议层用 `ts_vectors.rs`/`*.json` 做已知答案测试。
- 每个 `.rs`/`.slint` 文件首行是 SPDX 许可证头（`scripts/spdx-header.sh --check` 会校验）；Markdown、JSON、TOML、YAML 由 dprint 统一格式（`dprint.json`，行宽 120）。
- `rustfmt.toml`：edition/style 2024；`.editorconfig`：2 空格（Rust 4 空格）、LF、UTF-8、行尾去空白。
- `.githooks/pre-commit` 会对暂存的 `.rs` 跑 `scripts/rustfmt-staged.sh`（需 `git config core.hooksPath .githooks` 才生效）。
- 错误处理以 `Result<_, String>` 为主（面向日志/用户提示），协议层定义枚举错误（如 `iap2_csm::Error`、`FrameError`），少见 `anyhow`。

---

## 3. 运行时架构

### 3.1 进程全景

```
                         ┌─────────────────────────────┐
                         │        LIVI (livi-ui)       │  Slint 窗口
                         │  wayland 客户端 / 或宿主桌面 │
                         └──────────────┬──────────────┘
                                        │ Unix socket, 长度前缀 JSON (protocol 11)
                                        │ $XDG_RUNTIME_DIR/livi/core.sock
                         ┌──────────────▼──────────────┐
      ┌──────────────────┤          livi-core          ├───────────────────┐
      │ 二进制协议        │  状态 Hub / 会话 / 配置      │  行文本控制协议     │
      │ gst.sock          └───────┬─────────────┬─────┘  compositor.ctrl   │
      │                           │             │                          │
┌─────▼──────┐            ┌───────▼──────┐      │                  ┌───────▼────────┐
│livi-gst-host│            │ sudo -n -E   │      │                  │ livi-compositor│
│ GStreamer  │            │ livi-helperd │      │                  │ Wayland/DRM    │
│ 解码/音频   │            │  (root)      │      │                  │ 合成 + 输入     │
└─────┬──────┘            └──────┬───────┘      │                  └───────┬────────┘
      │ dma-buf (waylandsink)    │ /tmp/cp-bt.sock │                        │
      └──────────────────────────┼─────────────────┘                        │
                                 │ 蓝牙 / hostapd / dnsmasq / MFi / USB     │
                          ┌──────▼───────┐                          ┌───────▼────────┐
                          │ 手机 (CarPlay│                          │ DRM/KMS 屏幕    │
                          │ /Android Auto)│                         │ 或宿主窗口      │
                          └──────────────┘                          └────────────────┘
```

### 3.2 启动顺序（Linux）

`livi-core/src/main.rs` 的 `run()`：

1. 取 `euid`、计算路径（`paths.rs`）；把 runtime 目录权限设为 `0700`；对 `core.lock` 加锁，保证**单实例**。
2. 打开日志文件（用户数据目录下 `log/`）。
3. 读取主机信息：主机名 → 默认车名；EDID → 面板原生分辨率（`host.rs`）。
4. 载入 `config.json`：与默认值合并、修正类型/取值，必要时写回（`config_file.rs`）。
5. 读取上次运行的 UI 路径、DAB 状态（`state_files.rs`），决定启动页。
6. 建立状态中枢 `Hub`（完整 `State` + 版本号 + patch 广播）。
7. 初始化 `Core`，扫描 MFi 凭据（`mfi.rs`）。
8. 绑定 `core.sock`，接受 UI 连接（`server.rs`）。
9. 并发启动后台任务：LIVI Link 轮询、Wi-Fi 选项跟随、遥测、GNSS、车况桥、系统音量、更新、状态落盘、音频设备跟随。
10. `host_setup::prepare()`：写入 WirePlumber 的蓝牙角色配置（`a2dp_sink/source`），必要时重启 wireplumber。
11. `udev_rule::ensure()` / `kiosk_unit::ensure()`：模板版本变化时通过 helper 安装，前者要求重启，后者要求退出。
12. `Services::start()`：启动 helper（sudo）、compositor、GStreamer 宿主、UI，以及频谱、闪烁、摄像头等跟随任务。
13. `tokio::select!` 等待：UI 服务循环 / SIGINT/SIGTERM / 退出通知。
14. 收尾：停止 gvfs guard、向手机道别（`goodbye()`）、停止服务、释放 Wi-Fi AP、删除 socket；按需返回“退出/关机/重启/重新执行”。

macOS 上方向相反：**UI 先启动 core**（`livi-ui/src/core_process.rs`），因为 App 是窗口程序；`LIVI_LIFELINE` 管道保证一方退出另一方跟随。

### 3.3 进程职责与监督

| 进程       | 二进制            | 启动者             | 身份     | 监督策略                                                                  |
| ---------- | ----------------- | ------------------ | -------- | ------------------------------------------------------------------------- |
| core       | `livi-core`       | AppRun / UI(macOS) | 普通用户 | 单实例锁；由启动者通过 lifeline 监视（macOS 为 UI）                       |
| UI         | `LIVI`            | core（Linux）      | 普通用户 | 崩溃后 1 s 重启，最多 5 次；稳定 30 s 后计数清零（`ui.rs`）               |
| gst-host   | `livi-gst-host`   | core               | 普通用户 | 独立进程；崩溃写 `livi-gst-host-crash.log`（`services.rs`、`process.rs`） |
| compositor | `livi-compositor` | core               | 普通用户 | 退出即通知 core 退出（`watch_compositor`）                                |
| helper     | `livi-helperd`    | core，`sudo -n -E` | root     | 2 s 后重启，最多 5 次（`helper.rs`）                                      |

所有子进程都带 **lifeline**：core 持有子进程 stdin 管道，core 退出 → 管道关闭 → 子进程自杀；反之 UI/compositor 退出也会通知 core。这避免了 AppImage 被杀后残留 root helper。

### 3.4 进程间通信一览

| 通道              | 端点                                                                  | 编码                                    | 用途                                          |
| ----------------- | --------------------------------------------------------------------- | --------------------------------------- | --------------------------------------------- |
| UI ↔ core         | `$XDG_RUNTIME_DIR/livi/core.sock`（回退 `/tmp/livi-<uid>/core.sock`） | 4 字节小端长度 + JSON                   | 全量状态、增量 patch、输入、动作              |
| core ↔ gst-host   | `$runtime/gst.sock`                                                   | 4 字节本机长度 + `op:u8,id:u32,payload` | 建 plane、推码流、音频、麦克风、摄像头、URL   |
| core ↔ compositor | `$runtime/compositor.ctrl`                                            | 行文本                                  | 屏幕布局、plane 认领、视频裁剪/显示、模式切换 |
| core ↔ helper     | `/tmp/cp-bt.sock`                                                     | 行 JSON（二进制用 base64）              | 蓝牙、AP、MFi、USB、设备事件                  |
| 外部 → core       | TCP `:4000`，`POST /telemetry` + Socket.IO                            | JSON                                    | 车况遥测注入/订阅                             |
| core → 手机       | USB NCM / Wi-Fi / TCP                                                 | iAP2 / AA 协议                          | 投影会话                                      |

### 3.5 UI 协议（`livi-core-proto`）

- `PROTOCOL = 11`（`lib.rs`），版本不匹配会 `Refused`。
- 连接后 UI 发 `Hello { protocol, client }`，core 回 `Welcome { protocol, version, rev, state }`；随后 core 只发 `Patch { rev, ops }`，UI 本地应用 JSON Patch 风格的差异（`patch.rs`）。
- UI 上行消息：`Resync`、`Input`、`Action`、`Path`、`Spectrum`、`LinkSpeed`。
- 下行消息：`Welcome`、`Refused`、`Patch`、`Reply`、`Spectrum`、`OpenPage`。
- `Action` 覆盖设置修改、翻页、媒体控制、设备选择/忘记/连接、仪表布局、MFi 导入/删除、升级、DAB 扫描/播放、开关屏等。
- 帧上限 16 MiB（`frame.rs`）。

### 3.6 host 协议（`livi-host-proto`）

- 固定头：`len:u32(本机字节序)` + `op:u8` + `id:u32` + 负载。
- 主要操作码：`OP_CREATE/OP_DATA/OP_STOP`（视频 plane）、`OP_GAMMA`、`OP_LISTEN`（加密屏幕流）、`OP_AUDIO_*`、`OP_MIC_*`、`OP_VISUALIZER`、`OP_FEED_*`（AA 直喂）、`OP_TAP_*`、`OP_PLAY_*`、`OP_CAMERA_*`。
- plane 编号：`PLANE_MAIN=0x7a000001`、`PLANE_VIDEO=0x7a000002`、`PLANE_CAMERA=0x7a000003`、`PLANE_CLUSTER_RECV=0x7a000010`、`CLUSTER_PLANE_MIN..MAX=0x7a000011..13`。
- 屏幕标签：`main`、`video`、`camera`、`cluster-<screen>`。

### 3.7 compositor 控制协议

行文本命令，例如 `ui <wayland-socket> <node>`、`panel …`、`claim <tag>`、`unclaim <tag>`、`videocfg …`、`videoshow …`、`aspect …`、`mode …`、`screen …`、`fullscreen …`、`restart`（`livi-compositor/src/ctrl.rs`）。compositor 定时 16 ms 派发，向 core 回报面板与显示器列表。

### 3.8 helper 协议

- helper 端 `livi-runtime/src/livi_sock.rs` 提供 `Broadcaster`，把设备事件（`device`、`device-gone`、`wifi joined/left` 等）以 JSON 行推给 core。
- core 端 `livi-cp/src/helper_sock.rs` 是按行请求/响应的客户端（8 s 超时，失败自动重订阅）。
- helper 还提供一次性子命令：`--wifi-ap`、`--wifi-ap-status`、`--wifi-channels`、`--bt-tunnel`、`--install-udev-rule`、`--install-kiosk-unit`、`--install-gvfs-guard` 等（`livi-helperd/src/main.rs`）。

---

## 4. 投影协议栈

### 4.1 Apple CarPlay（`livi-cp` + `iap2-*` + `livi-runtime`）

#### 分层

```
应用语义   车辆状态/媒体/导航/气候/仪表主题        vehicle_state/、info.rs、ui_sync.rs
控制面     RTSP + binary plist + 控制加密           rtsp.rs、bplist.rs、control_cipher.rs
会话层     iAP2 帧/会话/CSM                          iap2-link、iap2-csm、stack/
认证层     MFi 协处理器 / 文件身份 / 网络认证        iap2-mfi、auth_setup.rs、mfi4/
配对层     pair-setup / pair-verify / TLV8 / SRP     pair_setup.rs、pair_verify.rs、srp.rs、tlv8.rs
传输层     USB NCM + usbmux / Wi-Fi + Bonjour        iap2-usbmux、iap2-wired、bonjour.rs
```

#### 有线链路

1. iPhone 插入后由 USB 枚举为 Apple 设备（`05ac`），并出现 NCM 网卡。
2. `iap2-usbmux` 通过 usbmux 找到设备，`iap2-wired::open_carkit` 打开 carkit 服务（锁屏也可用），得到控制通道。
3. helper 启动 `livi-runtime` 的 link driver，跑 iAP2 的 Detect/Negotiate，建立控制会话（ID 10）、EA 会话（11）、文件传输会话（12）。
4. `livi-cp` 做 MFi 认证、pair-verify，然后 RTSP `SETUP` 建立视频/音频流，进入会话主循环。

相关文件：`livi-helperd/src/wired.rs`（热插拔扫描 2 s、失败重试 3 次）、`livi-runtime/src/bringup.rs`、`livi-core/src/carplay.rs`。

#### 无线链路

1. helper 起蓝牙（Linux 走 BlueZ D-Bus），注册 Wireless iAP / CarPlay 服务记录（`livi-runtime/src/bt.rs`）。
2. 启用无线投影时，core 跟随配置启动 Wi-Fi AP（hostapd + dnsmasq，`livi-wifi`、`livi-runtime/src/wifi_ap.rs`；常驻场景由 `livi-wifi-ap.service` 提前拉起），手机经蓝牙完成配对与交接。
3. `livi-runtime/src/bonjour.rs` 用 `avahi-publish-service`（macOS 用 `dns-sd`）发布 `_airplay._tcp`，TXT 含 `deviceid`、`features=0x44540380,0x61`、`pk`、`pi`、`srcvers`；同时浏览 `_carplay-ctrl._tcp` 找到手机的地址。
4. 手机连上 AP 后，通过 iAP2 over Wi-Fi 隧道进入同一套 `livi-cp` 状态机；core 侧的 TCP 监听由 `livi-cp::net::tcp_listener()` 创建（双栈）。
5. 会话建立后，按 Apple 要求执行 `DisableBluetooth`，helper 断开该手机蓝牙，iAP2 完全走 Wi-Fi（`livi-cp/src/manager.rs` 的 `StackEvent::DisableBluetooth`）。

> 无线 CarPlay 需要蓝牙与 Wi-Fi 两个射频；两者都可以来自 LIVI Link（`wifiInterface = "CHOICE"` 时代码走 dongle 路径）。

#### 会话内的流

| 流           |  ID | 内容                |
| ------------ | --: | ------------------- |
| 主屏视频     | 110 | H.264/H.265，主界面 |
| 仪表视频     | 111 | 仪表/地图流         |
| 主音频       | 100 | 媒体                |
| 备用音频     | 101 | 提示音/语音         |
| 高优先级音频 | 102 | 通话/Siri           |
| 数据         | 130 | 控制、HID、车辆状态 |

音频支持 AAC-LC、Opus、PCM；视频支持 H.264/H.265（`livi-cp/src/stack.rs` 常量）。控制面还包含触摸/旋钮/按键 HID（`hid.rs`）、时间同步（`timing_sync.rs`）、封面图（`image_archive.rs`）、HLS 视频代理（`hls_gateway.rs`、`video_proxy.rs`）。

#### 多会话

`livi-cp/src/manager.rs` 维护多个 `SessionId`，每个会话保留自己的媒体、导航、电量、信号状态；切换只是把某个会话“拿到前台”，视频/音频通道不重建。

### 4.2 Android Auto（`livi-aa` + `livi-aa-stack` + `livi-aa-proto`）

#### 有线（USB AOAP）

1. `livi-aa/src/usb.rs` 轮询 USB：识别 Google 配件 PID `0x2d00..0x2d05` 与 17 个手机厂商 VID。
2. 对普通手机发 AOAP 控制请求：`51 GET_PROTOCOL`、`52 SEND_STRING`（品牌/型号/版本/URI/序列号）、`53 START`，手机重新枚举为 accessory。
3. 打开 USB 端点，得到字节流；在其上跑 TLS（`livi-aa/src/tls.rs`、`cert.rs`），再进入 WPP 帧解析（`wpp.rs`、`frame.rs`）。
4. `livi-aa-stack` 处理服务发现、通道打开、视频/音频/输入/传感器/导航/麦克风。

#### 无线

- 手机先通过蓝牙 HFP 连上车机（`livi-hfp` + `livi-runtime/src/bt.rs` 注册 AA UUID `4de17a00-52cb-11e6-bdf4-0800200c9a66`），蓝牙只用于“点火”。
- helper 起 AP（`livi-helperd/src/aa.rs`），把 SSID/密码/信道/BSSID 告诉 core；手机连 Wi-Fi 后主动连车机 TCP **5277**（`livi-aa/src/consts.rs`）。
- `livi-aa/src/server.rs` 接受连接，设置 `TCP_NODELAY` 与 5 s keepalive，然后走与有线相同的 TLS + WPP 会话（`session.rs`）。

#### 通道

`livi-aa-stack/src/channels/` 下按类型拆分：`video`、`audio`、`input`、`sensors`、`media_info`、`navigation`、`nav_maneuver`、`mic`。服务发现响应在 `discovery.rs` 中构造（50 个按键码、触摸屏、显示参数、编解码能力等）。

### 4.3 CarPlay 与 Android Auto 对比

| 维度         | CarPlay                                       | Android Auto                              |
| ------------ | --------------------------------------------- | ----------------------------------------- |
| 有线传输     | USB NCM + usbmux + carkit                     | USB AOAP（厂商私有协议切换）              |
| 无线传输     | 蓝牙配对 + Wi-Fi AP + Bonjour                 | 蓝牙 HFP 触发 + Wi-Fi AP + TCP 5277       |
| 认证         | MFi 协处理器/网络/文件身份                    | TLS 证书（`livi-aa/src/cert.rs`），无 MFi |
| 编码         | iAP2 + RTSP + plist                           | protobuf（prost）+ WPP                    |
| 无线频段要求 | 规范允许 2.4 GHz HT20（ch1/6/11），推荐 5 GHz | 实际要求 5 GHz                            |
| 多设备       | 有线/无线均可多台                             | 无线多台；有线目前 1 台                   |

---

## 5. 媒体管线

### 5.1 视频

```
手机码流 (H.264/H.265)
  → livi-core 解出 NAL / 码流配置
  → OP_CREATE(plane, codec, codec_data) + OP_DATA(plane, nal)
  → livi-gst-host: appsrc → <parser> → <decoder 候选链> → waylandsink(sync=false)
  → wl_buffer(dma-buf) → livi-compositor 导入 GLES 纹理
  → DRM/KMS plane 或宿主窗口
```

- 解码器候选（`livi-video/src/codec.rs`）：
  - H.264：`v4l2slh264dec` → `v4l2h264dec` → `vah264dec` → `avdec_h264`
  - H.265：`v4l2slh265dec` → `v4l2h265dec` → `vah265dec` → `avdec_h265`
  - VP9/AV1：对应硬解优先，否则软件解码
  - NVIDIA 上优先 `nv*dec`；VA-API 可指定 render node（`va<node>…`）
  - macOS 用 `vtdec` → `avdec_*`
- sink：Linux 默认 `waylandsink name=sink sync=false`，可用 `LIVI_GST_SINK` 覆盖；macOS 为 `glimagesink`。
- 强制软解：`LIVI_GST_SWDEC=1`。
- 画面裁剪/安全区：`livi-media/src/planes.rs` 的 `crop_for()` 计算保持宽高比的可见区域，交给 compositor 做裁剪与居中。

### 5.2 音频

下行（手机 → 车机），`livi-audio/src/player.rs`：

```
appsrc → (rtpjitterbuffer → rtp*depay → 解码) → audioconvert → clocksync
       → volume → audioconvert → audioresample
       → audio/x-raw,S16LE,48000,2ch → queue → pulsesink device=<配置>
```

- 通话/媒体/提示音/语音助手各有独立 `Player` 与音量（`Config::audio_volume`、`nav_volume`、`voice_assistant_volume`、`call_volume`）。
- 输出设备由 `audioOutputDevice` 指定；`huVolumeLinkSystem` 让主机音量键直接改系统 sink 音量（`livi-core/src/system_volume.rs`）。
- 上行（麦克风），`livi-audio/src/player/uplink.rs`：`pulsesrc` → 编码（Opus/AAC/PCM）→ RTP → 手机；Android Auto 通话另有 SCO 路径（`/tmp/aa-sco.mic`，8 kHz 单声道）。
- 频谱：`livi-gst-host/src/spectrum.rs` 从音频缓冲取 24 个频带，经 `FromCore::Spectrum` 推给 UI。

### 5.3 摄像头与 DAB+

- 摄像头：`livi-video-player/src/camera.rs` 枚举 V4L2 设备，`OP_CAMERA_OPEN` 建 plane；倒车触发由 `auto_switch_on_reverse` 控制。
- DAB+：`livi-rtlsdr` 读 USB SDR → `livi-dab` 解调（dev profile 也对 `livi-dab`、`rustfft` 开 `opt-level=3`）→ 音频/台标/电台文本进 UI。

### 5.4 显示校正

`livi-compositor` 在最终合成时应用 gamma/对比度/RGB 增益（`livi-host-proto/src/display.rs::Correction`）。默认值下整条校正路径被跳过，不产生额外开销。校正覆盖 UI、仪表盘和投屏视频。

---

## 6. UI 层

### 6.1 渲染结构

- `livi-ui` 是 Slint 应用（`ui/app.slint` 及 22 个 `.slint` 文件），使用 femtovg 渲染器（GL）或 software 后端。
- `build.rs` 编译 Slint 并生成图标表（161 个 SVG → `include_bytes!`），同时为 macOS 写入 GStreamer 的 rpath。
- Linux 下 UI 作为 `livi-compositor` 的 Wayland 客户端运行；compositor 直接把窗口全屏铺到 DRM 输出（kiosk）或作为宿主桌面的一个窗口（嵌套模式）。
- macOS 下 UI 直接开原生窗口，视频由 `livi-video-view` 的视图承载。

### 6.2 UI 模型（`livi-ui-model`）

- `shell.rs`：决定标签页（投影/仪表/媒体/摄像头/设置）、主题色、是否右舵、UI 缩放、投屏流尺寸、语言。
- `settings/`：一棵由 `Node` 构成的设置树，`schemas/` 下按 `devices/general/car/audio/video/appearance/system` 分组；每个控件读写 `config.json` 的键，并订阅 `STATE_READ` 中的运行状态。
- `dashboard/`：仪表盘布局、仪表、导航卡片、读数格式化。
- `i18n.rs`：`locales/{en,de,fr,ua,zh}.json` 扁平化查找，缺词回退英文；`zh` 为本分支新增的 357 行简体中文。
- 字体：Roboto 四个字重 + 本分支新增的 `NotoSansSC-LIVI.ttf`（约 5.5 MB，随 AppImage 打包，`SLINT_FONT_PATH` 指向 `resources/fonts`）。

### 6.3 页面与交互

- 主界面五个标签：Projection、Telemetry（仪表盘）、Media、Camera、Settings；标签可按配置隐藏（`shell.rs::tabs`）。
- 二级窗口（Dash/Aux）由 `secondary.rs` 管理，可显示仪表、倒车影像或媒体；分配不互斥。
- 覆盖层：音量、Siri/通话、通知、升级提示（`overlay_ui.rs`）。
- 输入：触摸（`touch_ui.rs`）、旋钮/方向键（`keys_ui.rs`）、硬键映射（`Config::key_bindings`），以及投屏内的触摸坐标变换（考虑裁剪区域，`livi-compositor/src/projection.rs`）。
- 缩放：`ui_scale(base, zoom, physical_height)` 在屏幕过矮时自动缩小，避免控件溢出。

---

## 7. 配置、状态与持久化

### 7.1 `config.json`

| 项目     | 位置                                                                        |
| -------- | --------------------------------------------------------------------------- |
| 主配置   | `$XDG_CONFIG_HOME/LIVI/config.json`（默认 `~/.config/LIVI/config.json`）    |
| 备份副本 | `$XDG_DATA_HOME/LIVI/config.json`（默认 `~/.local/share/LIVI/config.json`） |
| 运行目录 | `$XDG_RUNTIME_DIR/livi/`（回退 `/tmp/livi-<uid>`）                          |
| 用户数据 | `~/.config/LIVI/`（`cp/`、`mfi/`、`driver/`、`log/`、`state/` 等）          |

加载逻辑（`livi-core/src/config_file.rs`）：

- 主文件不存在则读备份；两者都没有则用默认值。
- 与默认值做**按类型的合并**：未知键丢弃、类型不符回退默认值，保证升级后旧配置仍可用。
- `wifiPassword` 不在 8–63 字符范围内时回退默认值并告警。
- 若未显式设置 `projectionWidth/Height`，按 EDID 面板分辨率**等比缩到不超过 1280×720**（`fit_720p`），并同时作为仪表流默认值。
- 启动时若规范化后的配置与文件不同，立即原子写回。

主要配置分组（`livi-core-proto/src/config.rs`）：

| 分组      | 代表键                                                                                                                                                                     |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 无线      | `wirelessCpEnabled`、`wirelessAaEnabled`、`wifiPassword`、`wifiInterface`、`wifiDedicatedInterface`、`wifiType`、`wifiChannel`、`wifiChannelWidth`、`country`、`btAdapter` |
| CarPlay   | `carPlayMfiI2cBus`、`carPlayMfiPowerGpio`、`carPlayMfiFiles`、`carPlaySourceVersion`、`carPlayHlsVideo`、`carPlayDockPosition`、`carPlayTheme`、`carPlayCluster*`          |
| 投屏/仪表 | `projection{Width,Height,Fps,Dpi}`、四周 view/safe area、`cluster*` 同名字段                                                                                               |
| 音频      | `audioOutputDevice`、`audioInputDevice`、四路音量、`huVolume`、`huVolumeLinkSystem`、`visualAudioDelayMs`                                                                  |
| 显示      | `darkMode`、`displayGamma/Contrast/ColorR/G/B`、`displayMode`、`displayConnector`、`kiosk`                                                                                 |
| UI        | `language`、`uiZoomPercent`、`startPage`、`keyBindings`、`tabSettings`                                                                                                     |
| 车辆      | `carType`、`evConnectorTypes`、`maxSpeedKph`、`maxRpm`、`redlineRpm`、`hand`、`speedUnit`、`temperatureUnit`                                                               |
| GPS       | `gpsEnabled`、`gpsDevice`、`gpsBaudRate`、`timezone`                                                                                                                       |
| 其他      | `autoConn`、`autoSwitchOnReverse`、`samplingFrequency`、`disableAudioOutput`、DAB 增益/PPM                                                                                 |

### 7.2 状态中枢

`livi-core/src/hub.rs` 用 `Mutex<State>` + `watch` + `broadcast`：

- 每次修改递增 `rev`，并把**新旧 JSON 的差异**（`patch::diff`）广播给所有 UI。
- 新 UI 连接拿到完整 `Welcome`，之后只收 patch，因此重连/多窗口都很轻。
- `Hub::applied()` 单独跟踪“已生效的配置”，用于判断哪些改动需要重启 helper/重建管线。

### 7.3 状态文件与日志

| 数据                  | 位置                                                      |
| --------------------- | --------------------------------------------------------- |
| UI 路径、DAB 上次电台 | `state/`（`state_files.rs`）                              |
| 气候状态              | `state/climate.json`                                      |
| GPS 镜像              | `state/gps.json`                                          |
| MFi 凭据              | `mfi/certificate.p7b`、`mfi/identity.pk8`（0700/0600）    |
| CarPlay 身份/配对     | `cp/identity.json`、`cp/pairings.json`                    |
| 日志                  | `log/`（core、gst-host、compositor），另见 `DEBUGGING.md` |

### 7.4 遥测入口

`livi-core/src/telemetry.rs` 在 **4000** 端口同时提供：

- `POST /telemetry`：直接推 JSON（`curl -d '{"gear":"R"}' http://<host>:4000/telemetry`）；
- Socket.IO：订阅/推送 `telemetry:update`、`telemetry:push`；
- `livi-core telemetry set/demo` 子命令（`telemetry_cli.rs`）。

监听地址为 `[::]:4000` 或 `0.0.0.0:4000`，代码中未做鉴权——适合车内局域网，不应暴露到不可信网络。

### 7.5 自更新

`livi-core/src/update.rs` 查询 GitHub Releases（或 `UPDATE_FEED`/`UPDATE_REPO`），按平台挑选 `.AppImage`/`.dmg` 资源，下载并安装，进度通过状态 `update` 推给 UI。

---

## 8. 构建与发布流程

### 8.1 构建依赖

**通用**：Rust（rustup 按 `rust-toolchain.toml` 安装固定版本）、`build-essential`、`pkg-config`、`cmake`（aws-lc）、`libssl-dev`、GStreamer 开发包、Wayland/xkbcommon/fontconfig、libudev/libinput/libseat/libgbm。

Debian/Ubuntu：

```bash
sudo apt-get install -y git build-essential pkg-config cmake libssl-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libwayland-dev libxkbcommon-dev libfontconfig1-dev \
  libudev-dev libinput-dev libseat-dev libgbm-dev \
  fuse3 bluez libspa-0.2-bluetooth hostapd dnsmasq-base iw rfkill \
  avahi-daemon avahi-utils pulseaudio-utils
```

运行时还需要：`hostapd`、`dnsmasq`、`iw`、`rfkill`、BlueZ、PipeWire/WirePlumber（含蓝牙插件）、`avahi`（无线 CarPlay 发布 Bonjour）。Fedora 的包名差异见 README。

打包额外需要：`appimagetool`（Linux，`APPIMAGETOOL` 可指定路径）、`desktop-file-validate`；macOS 需要 GStreamer framework 与 Xcode 命令行工具。

### 8.2 从源码到 `out/bin`

在 `native/` 下执行：

```bash
cargo xtask build-native            # 本机架构
cargo xtask build-native --arch arm64
```

`native/.cargo/config.toml` 把 `cargo xtask` 映射为 `run --quiet --package xtask --`。`build_native.rs` 的行为：

1. 通过 `cargo metadata` 找 target 目录。
2. 以 `--release` 构建：`livi-gst-host`、`livi-helperd`、`livi-core`、`livi-ui`（Linux 另加 `livi-compositor`）；CI 中加 `--locked`。
3. 注入版本环境变量：`LIVI_VERSION`、`LIVI_BUILD_DATE`、`LIVI_BUILD_SHA`、`LIVI_BUILD_RUN`，并设 `PKG_CONFIG_ALLOW_CROSS=1`；macOS 自动加 GStreamer 的 pkgconfig 路径。
4. 跨架构时先 `rustup target add` 再加 `--target`。
5. 把产物复制为 `out/bin/{livi-gst-host,livi-helperd,livi-core,LIVI,livi-compositor}`（`livi-ui` 的二进制改名为 `LIVI`）。

版本号唯一来源：`native/Cargo.toml` 的 `[workspace.package] version`（当前 `10.1.0`）。`xtask` 与 CI 都从这里读。

### 8.3 Linux 打包（AppImage）

```bash
cd native
cargo xtask package-linux --arch arm64
# 产物：dist/LIVI-10.1.0-linux-arm64.AppImage
```

流程（`native/xtask/src/package_linux.rs`）：

1. 先跑 `build-native`。
2. 组装 `dist/linux-<arch>-unpacked/`：

```
LIVI-<version>-linux-<arch>.AppImage
└── (挂载后)
    ├── AppRun -> LIVI                 # 符号链接
    ├── LIVI                           # launcher 脚本
    ├── livi-ui                        # 真正的 Slint UI 二进制
    ├── livi.png
    ├── dev.f-io.livi.desktop
    ├── share/metainfo/dev.f-io.livi.metainfo.xml
    └── resources/
        ├── core/livi-core
        ├── driver/livi-helperd
        ├── gst-host/livi-gst-host
        ├── compositor/livi-compositor
        ├── fonts/NotoSansSC-LIVI.ttf
        ├── gstreamer/linux-<arch>/     # 自带 GStreamer 运行时
        ├── displays/*.edid
        ├── 99-LIVI.rules.template
        ├── 99-LIVI-helper.sudoers.template
        ├── 99-LIVI-wifi-ap.sudoers.template
        ├── livi-wifi-ap.service.template
        ├── livi-kiosk.service.template
        ├── livi-touch-filter
        └── setup-hdmi-pr-display.sh
```

3. 写启动脚本（`START` 常量），设置 `LIVI_RESOURCES`、`SLINT_FONT_PATH`，然后 `exec resources/core/livi-core`；`telemetry` 子命令直接透传给 core。
4. 调 `appimagetool --no-appstream` 生成 AppImage。

注意：`assets/gstreamer/linux-<arch>/` 必须存在，否则打包会在复制阶段失败。它由 8.5 的流程生成并随仓库提供。

### 8.4 macOS 打包

```bash
cargo xtask package-mac              # arm64
cargo xtask package-mac --arch x64   # Intel / Rosetta 构建
```

`package_mac.rs` 生成 `dist/LIVI.app`：`Contents/MacOS/LIVI`、`Contents/Resources/{core,driver,gst-host,gstreamer/macos-arm64}`、`Info.plist`（含相机/麦克风/本地网络权限说明）、`livi.icns`。随后：

- 用 `install_name_tool` 重写 rpath，指向 App 内的 GStreamer；
- 对嵌套 Mach-O 逐个签名（可执行文件用 inherit entitlements），再签整个 App，`codesign --verify --deep --strict`；
- 用 `ditto` 组 DMG 目录，`diskutil image create`（不可用时回退 `hdiutil`）生成 `dist/LIVI-<version>-macos-<arch>.dmg`。

### 8.5 自带 GStreamer（为什么要自己编）

系统 GStreamer 往往缺嵌入式硬解或版本不匹配，LIVI 因而在 `assets/gstreamer/` 里提交一份预编译运行时（约 538 MB），由 `.github/workflows/gstreamer-assets.yml` + `scripts/gstreamer/package-linux.sh` 生成。

补丁集中在 `scripts/gstreamer/patches/gst-plugins-bad` 与 `gst-plugins-base`，主题是**嵌入式视频格式与零拷贝**：

- `v4l2codecs`：更新 bit-size、支持 `V4L2_PIX_FMT_NC12`、新增 `NV12_128C8`/`NV12_10L32_128C8` 等常用平铺（tile）格式；
- Wayland/dmabuf：按 column-mode 正确计算 tile stride；
- `waylandsink`：输出属性更新不再崩溃；
- `vtdec`（macOS）：HEVC reorder、full-range 输出修复。

运行时通过 `resources.rs::gst_env()` 注入 `GST_PLUGIN_PATH`、`GST_PLUGIN_SCANNER`、`LD_LIBRARY_PATH`（macOS 为 `DYLD_LIBRARY_PATH`），并清空 `GST_PLUGIN_SYSTEM_PATH`，避免和系统插件混用。若宿主 `libva` 过旧，会额外挂 `libva-compat`。

### 8.6 arm64 交叉构建（x86_64 主机）

```bash
scripts/linux-arm64/build.sh /path/to/LIVI [cache-dir]
```

`scripts/linux-arm64/Containerfile` 基于 Debian Trixie，安装 `gcc-aarch64-linux-gnu` 与 arm64 版开发包，设置 `PKG_CONFIG_LIBDIR`、`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER`，在容器内执行 `cargo xtask package-linux --arch arm64`。Cargo registry/git/rustup/target 都挂载为缓存卷。

### 8.7 在本板（RK3566 / arm64 Linux）上构建

如果开发机本身就是 arm64 Linux（例如板子），可以直接原生构建，步骤与 8.1/8.3 相同；额外注意：

- `appimagetool` 需要 aarch64 版本；无 FUSE 时设 `APPIMAGE_EXTRACT_AND_RUN=1`。
- 仓库已有 `out/bin/`（aarch64 动态链接）与 `dist/LIVI-10.1.0-linux-arm64.AppImage`，说明本机已完成过一次 `package-linux --arch arm64`。
- 具体依赖安装、离线 MFi、显示适配与中文支持的记录见 [RK3566-DEVELOPMENT.md](RK3566-DEVELOPMENT.md)。

**不打包、直接跑源码树**也可以：`cargo xtask build-native` 后执行 `out/bin/livi-core`。`Resources::repo()` 会从可执行文件向上找到含 `native/Cargo.toml` 的目录，使用 `out/bin` 与 `assets/gstreamer`、`assets/linux`。常用调试变量：

```bash
LIVI_NO_UI=1        ./out/bin/livi-core     # 只跑 core
LIVI_UI_CMD=...     ./out/bin/livi-core     # 换 UI
LIVI_UI_STATE=...   ./out/bin/LIVI          # 离线渲染状态快照
```

### 8.8 CI 工作流

| 工作流                 | 触发                          | 内容                                                                                                                                                                                                                                                                  |
| ---------------------- | ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `checks.yml`           | push 任意分支、PR 到 main/dev | Linux：`cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings`、`cargo test --workspace`；macOS：同款 clippy/test（排除 compositor、livid 等 Linux-only crate）；独立 lint job 跑 dprint、actionlint 和 SPDX 头检查；额外对 `livid` 做 armv7 clippy |
| `build.yml`            | checks 在 main 成功后 / 手动  | 矩阵构建 linux x64/arm64 AppImage、macOS arm64/x64 DMG，上传 artifact，并更新 version 分支的徽章                                                                                                                                                                      |
| `gstreamer-assets.yml` | 手动                          | 重建三平台 GStreamer 运行时并提交到 `assets/gstreamer`                                                                                                                                                                                                                |
| `livi-link.yml`        | 手动                          | 7 个 dongle 固件目标、共享 crate 测试、`.lfwb` 打包冒烟、bind-shell 重建                                                                                                                                                                                              |
| `release.yml`          | 手动                          | 从某次成功 build 取产物，创建 GitHub Release（可草稿）                                                                                                                                                                                                                |
| `dependencies.yml`     | 手动                          | `cargo upgrade` 更新依赖并开 PR/提交                                                                                                                                                                                                                                  |
| `maintenance.yml`      | 每日                          | 清理旧 workflow run 与缓存                                                                                                                                                                                                                                            |
| `notification.yml`     | release                       | 向 Discord 推送发布信息                                                                                                                                                                                                                                               |

CI 的 Linux runner 直接使用 `ubuntu-24.04` / `ubuntu-24.04-arm`；macOS 使用 `macos-latest`；Cargo 缓存以 `Cargo.lock` + 所有 `Cargo.toml` 的哈希为键。

### 8.9 发布流程

1. 修改 `native/Cargo.toml` 的 workspace 版本。
2. 等 `checks` 绿、`build` 出四平台产物（或手动触发 `build`）。
3. 手动跑 `release`，选择 build run 或最新 main 成功构建，创建 tag（`v<version>`）与 Release。
4. Release 附件即 AppImage/DMG；`notification` 同时推 Discord。

### 8.10 LIVI Link 固件构建（独立线）

LIVI Link 是另一条完整的产品线，不参与 `cargo xtask`：

- 板级脚本：`scripts/livi-link/{arm,riscv,mips}/<board>/build.sh`，负责内核（含补丁）、DTS、U-Boot/启动镜像、initramfs；
- 用户态：`scripts/livi-link/arm/build-userspace.sh` 交叉编译 musl、busybox、libnl3、hostapd（全静态）；
- rootfs：`scripts/livi-link/common/build-rootfs.sh` 组装 squashfs，把 `livid` 以多调用方式安装为 `livi-netd/livi-httpd/livi-wifid/livi-ledd/livi-bt-up/livi-mfid/livi-btd/livi-accessoryd`；
- 打包：`pack-bundle.sh` 生成 `.lfwb`，`stage-firmware.sh` 放入 `assets/livi-link/`（由 CI 提交）。

`native/.cargo/config.toml` 为 `riscv32gc-unknown-linux-gnu` 与 `mipsel-unknown-linux-musl` 指定了静态链接器与 rustflags。

### 8.11 安装脚本与开机自启

入口 `scripts/install/install.sh` 根据 `systemctl get-default` 判断桌面还是 headless，再交给对应流程；`common.sh` 提供共用函数。

**headless（车机/kiosk）流程**：

1. 安装软件包（`livi_packages core lite`），把用户加入 `video/render/input/plugdev`。
2. 下载 AppImage 到 `~/LIVI/LIVI.AppImage`（可选本地文件/URL）。
3. 从 AppImage 中提取模板（udev、sudoers、kiosk unit、触摸过滤）。
4. 写 `/etc/udev/rules.d/99-LIVI.rules`、sudoers drop-in、触摸过滤脚本、无线管制域、时间/电源辅助脚本。
5. 询问 MFi（I²C 总线/文件凭据），写入 `config.json`。
6. 配置 tty1 自动登录 + `livi-kiosk.service`（`Conflicts=getty@tty1.service`，`Restart=no`）与 PAM。

**freedesktop 桌面流程**类似，但使用 autostart 而不是 kiosk unit。

`livi-kiosk.service` 模板由 core 在运行时自检：模板带 `LIVI-KIOSK-VERSION`，版本变化时通过 helper 重写 unit（`livi-core/src/kiosk_unit.rs`）。udev 规则同理（`LIVI-RULE-VERSION`，当前 6）。这正是“AppImage 直接替换后仍能升级系统集成”的机制。

---

## 9. 测试、调试与质量保障

### 9.1 测试

- 全仓库约 **1,353 处** `#[test]`/`#[tokio::test]`，分布在 272 个文件中；以模块内单元测试为主。
- 协议层有**已知答案测试**：`livi-cp/src/ts_vectors.rs`、`livi-aa-stack/src/ts_vectors.rs`、`livi-crypto` 的 RFC 8439 向量、`livi-wifi` 的频段/信道测试等。
- `iap2-mfi/src/testdata/` 有测试用证书/身份（`.gitignore` 中单独放行），用于离线 MFi 解析测试。
- UI 模型有约束性测试，例如“设置页读取的每个状态键都必须登记在 `STATE_READ` 中”，防止漏刷新；还有 `Resources::repo()` 路径布局测试、`xtask` 版本/日期格式测试。
- CI 命令：

```bash
cd native
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo clippy -p livid --target armv7-unknown-linux-musl --locked -- -D warnings

cd ..
dprint check                        # Markdown/JSON/TOML/YAML
sh scripts/spdx-header.sh --check   # .rs/.slint 许可证头
```

- 日常开发可只跑目标 crate：`cargo test -p livi-ui-model`、`cargo test -p livi-core nav_text`（见 RK3566 文档 3.3）。

### 9.2 调试开关（摘要）

| 变量                                               | 作用                                                                                  |
| -------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `DEBUG=1` / `TRACE=1`                              | 全局调试/跟踪日志（helper 也会开启）                                                  |
| `LIVI_GST_DEBUG=1`                                 | GStreamer 分类日志（`v4l2codecs-*:6,waylandsink:5,wl_dmabuf:6`）；也可传任意 debug 串 |
| `LIVI_GST_SWDEC=1`                                 | 强制软件解码                                                                          |
| `LIVI_GST_SINK=<element>`                          | 替换视频 sink                                                                         |
| `LIVI_GST_PRELOAD=<lib>`                           | 仅对 gst-host 做 `LD_PRELOAD`                                                         |
| `LIVI_WLR_DEBUG=1`                                 | compositor 详细日志                                                                   |
| `LIVI_DEBUG_BG=1`                                  | 合成器画品红背景，检查布局/plane                                                      |
| `LIVI_NO_COMPOSITOR=1`                             | 不启动合成器                                                                          |
| `LIVI_NO_UI=1` / `LIVI_UI_CMD`                     | 不启动 UI / 换 UI                                                                     |
| `LIVI_UI_SIZE`、`LIVI_UI_PAGE`、`LIVI_UI_SNAPSHOT` | 指定窗口尺寸、起始页、截图                                                            |
| `LIVI_UI_STATE=<json>`                             | 无 core 时用固定状态渲染 UI                                                           |
| `LIVI_CP_DEBUG=1`、`LIVI_CP_SYSLOG=1`              | CarPlay/helper 详细日志                                                               |

日志位置：stdout/stderr（kiosk 下进 journal）、`~/.config/LIVI/log/`、`livi-gst-host-crash.log`（AppImage 同目录）。

### 9.3 常见故障定位

| 现象              | 先看                                           | 再试                                                     |
| ----------------- | ---------------------------------------------- | -------------------------------------------------------- |
| 投屏黑屏          | gst-host 日志、`[gst-host]` 统计               | `LIVI_GST_DEBUG=1`；`LIVI_GST_SWDEC=1` 区分硬解/码流问题 |
| 画面位置/黑边不对 | compositor 日志、视图区域配置                  | `LIVI_DEBUG_BG=1` 看 plane 布局                          |
| 触摸偏移          | `livi-compositor/src/projection.rs` 的裁剪换算 | 检查 view area 与 UI 缩放                                |
| CarPlay 不连      | helper 日志、MFi 状态                          | 确认 `mfi/` 凭据、检查 USB udev 规则是否安装             |
| 无线 AP 起不来    | `[wifi-ap]` 日志、`iw phy`                     | 5 GHz 配置在 2.4 GHz 网卡上会回退失败（见 11.5）         |
| UI 起不来         | core 的 `[core] the UI …`                      | 确认 `out/bin/LIVI` 或 AppImage 内 `livi-ui`             |

---

## 10. 安全模型

### 10.1 权限分层

- `livi-core`、UI、gst-host、compositor 都以登录用户运行，不持有 root。
- 只有 `livi-helperd` 以 root 运行，通过 `sudo -n -E` 启动；sudoers drop-in 只允许**一个固定路径**的 helper（`~/.config/LIVI/driver/livi-helperd`），并允许 `SETENV`。
- helper 从 AppImage 内被**复制并校验摘要**到用户数据目录（`helper.rs::resolve_bin/stage`），避免直接执行只读挂载里的文件，也保证升级后旧 helper 能被替换。
- 残留的旧 helper 会被 `pkill -f driver/livi-helperd$` 清掉。
- helper 的子命令各自最小化权限：装 udev 规则、装 kiosk unit、起 AP、起蓝牙隧道。

### 10.2 设备与系统集成

`assets/linux/99-LIVI.rules.template` 做四类事：

1. 多点触控设备同时暴露鼠标接口时，用 `livi-touch-filter` 让 libinput 忽略鼠标接口。
2. Apple 设备（`05ac`）：禁用 gphoto/PTP/udisks 自动挂载，保证 usbmux 独占。
3. Apple NCM 网卡：关闭 IPv6 DAD（重复地址检测），避免 CarPlay 链路等待。
4. Android 厂商（三星、Google、OPPO、OnePlus、华为、小米等）：禁止桌面把它当 MTP 媒体设备抢走。

### 10.3 MFi 凭据

- `mfi/certificate.p7b`、`mfi/identity.pk8`，目录 0700、文件 0600；导入时先校验格式与长度（`livi-core/src/mfi.rs`、`iap2-mfi::file`）。
- `.gitignore` 忽略 `*.p7b`、`*.pk8`、`offline-mfi/`，仅放行测试向量。
- 配置里只保存凭据摘要（`carPlayMfiFiles`），不保存内容。

### 10.4 加密

- CarPlay：pair-setup/verify（SRP + TLV8）、控制通道加密（`control_cipher.rs`）、MFi 签名。
- Android Auto：TLS，使用内置的 Automotive Link 证书/私钥（`livi-aa/src/cert.rs`、`tls.rs`）。
- 无线屏幕流（LIVI Link 场景）：ChaCha20-Poly1305（`livi-crypto`、`livi-screen-stream`），带 128 字节头与 16 字节 tag。

### 10.5 网络面

- 遥测 `:4000`、AA TCP `5277`、CarPlay 监听端口默认绑定所有接口；这些协议假定运行在车内局域网。不要把车机直接暴露到公网。
- LIVI Link 的 Web 管理界面在 dongle 侧（`10.10.10.1` / `livi-link.local`）。

---

## 11. 二次开发指南

### 11.1 增加一个设置项

以 `wifiChannelWidth` 为例，一条配置要走通四处：

1. **配置模型**：`native/crates/livi-core-proto/src/config.rs` 加字段 + 默认值。
2. **设置树**：`native/crates/livi-ui-model/src/settings/schemas/<分组>.rs` 加控件，绑定同一个键。
3. **使用方**：core/helper 读取该键（`helper.rs::settings_env` 或 `linux_main.rs`），必要时加入“生效需重启 helper”的判断（`starts_differently`）。
4. **文案**：`livi-ui-model/locales/*.json` 加标签；UI 读取的状态键要登记到 `settings/mod.rs::STATE_READ`。

### 11.2 增加一个页面/窗口

- 页面：`ui/app.slint` 加组件 + `livi-ui/src/main.rs` 的 `Page` 枚举与 `tab()` 映射；`ui-model/shell.rs::tabs()` 决定是否显示。
- 二级窗口：参考 `livi-ui/src/secondary.rs` 与 `livi-core/src/projection/screens.rs`，窗口类型由 compositor 的屏幕标签决定。

### 11.3 修改视频/音频行为

- 编解码器优先级：`livi-video/src/codec.rs`（纯函数，容易加测试）。
- 管线字符串：`livi-audio/src/player.rs`（音频）、`livi-video-player/src/lib.rs`（视频）、`livi-video/src/codec.rs::sink_chain`。
- 画面裁剪/缩放：`livi-media/src/planes.rs::crop_for` 与 `livi-compositor/src/projection.rs`。

### 11.4 增加协议能力

- CarPlay：`livi-cp/src/stack/` 按 setup/commands/control/data_streams/events/feedback/teardown/video 拆分；新命令加在 `commands.rs`，并在 `vehicle_state/` 补状态映射。
- Android Auto：`livi-aa-proto` 加 protobuf，`livi-aa-stack/src/channels/` 加通道处理，`discovery.rs` 声明能力。

### 11.5 常见坑

| 坑                        | 说明                                                                                                                                      |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| kiosk 重启方式            | 本板 `systemctl restart livi-kiosk` 与 getty 冲突，应 `stop` 再 `start`（见 RK3566 文档）                                                 |
| 5 GHz 配置 + 2.4 GHz 网卡 | `after_refusal()` 对 5 GHz 只会回退到 `SAFE_CHANNEL_5`，会无限重启；必须把 `wifiType/wifiChannel/wifiChannelWidth` 改成 2.4 GHz/1·6·11/20 |
| 设置页允许选 5 GHz        | `bands_in()` 无论硬件如何都返回 2.4/5 GHz，UI 不会阻止                                                                                    |
| 开 AP 抢走 `wlan0`        | `release_iface_from_nm()` 会把接口从 NetworkManager 摘掉；SSH 走 Wi-Fi 时开无线投影会断连                                                 |
| AppImage 内资源路径       | 打包后一切从 `LIVI_RESOURCES` 走；源码树运行则用 `out/bin` + `assets/`                                                                    |
| 单实例                    | `~/.local/share`/runtime 下的 `core.lock` 被占用时新 core 直接报错                                                                        |
| GStreamer 版本            | 不要混用系统插件；`gst_env()` 已清空系统插件路径                                                                                          |

---

## 12. 已知限制与技术债

- **仪表盘仍在开发中**：遥测 IPC 支持很多信号，UI 只暴露一部分（README 明示）。
- **有线 Android Auto 限一台**；CarPlay 与无线 AA 支持多台。
- **无线 AA 需 5 GHz**；本板 AP6212A1 仅 2.4 GHz，实际不可用（详见 4.3 与 RK3566 文档）。
- **无线 CarPlay 在 2.4 GHz 上处于规范边缘**：规范允许但要求 ≥25 Mbps、<16 ms；单流 HT20 实际吞吐有限。
- **macOS 不支持 Basic Accessory Authentication**，也不支持板载 I²C MFi（无 I²C），只能用 LIVI Link。
- **HLS 视频代理**（`carplayHlsVideo`）与部分车辆功能依赖手机侧能力，兼容性随 iOS 版本变化。
- **UI 与投影分辨率共用** `projectionWidth/Height`，无法单独为无线降分辨率。
- **`livi-dab` 与 `livi-rtlsdr` 只支持特定 RTL-SDR 硬件**（RTL2832U + R820T2）。

---

## 附录 A：关键常量速查

| 名称              | 值                                                                    | 位置                                                          |
| ----------------- | --------------------------------------------------------------------- | ------------------------------------------------------------- |
| UI 协议版本       | `11`                                                                  | `livi-core-proto/src/lib.rs`                                  |
| core socket       | `$XDG_RUNTIME_DIR/livi/core.sock`（回退 `/tmp/livi-<uid>/core.sock`） | `core-proto/src/runtime.rs`                                   |
| gst-host socket   | `$runtime/gst.sock`                                                   | `livi-core/src/services.rs`                                   |
| compositor 控制   | `$runtime/compositor.ctrl`                                            | `livi-core/src/services.rs`                                   |
| helper socket     | `/tmp/cp-bt.sock`                                                     | `livi-runtime/src/livi_sock.rs`、`livi-cp/src/helper_sock.rs` |
| 遥测 HTTP         | `:4000`，`POST /telemetry`                                            | `livi-core/src/telemetry.rs`                                  |
| AA TCP            | `5277`                                                                | `livi-aa/src/consts.rs`                                       |
| CarPlay 视频流    | 110（主屏）、111（仪表）                                              | `livi-cp/src/stack.rs`                                        |
| CarPlay 音频流    | 100、101、102                                                         | 同上                                                          |
| CarPlay 数据流    | 130                                                                   | 同上                                                          |
| plane ID          | `0x7a000001/2/3`，cluster `0x7a000010..13`                            | `livi-host-proto/src/lib.rs`                                  |
| Bonjour 服务      | `_airplay._tcp`、`_carplay-ctrl._tcp`                                 | `livi-runtime/src/bonjour.rs`                                 |
| AA 蓝牙 UUID      | `4de17a00-52cb-11e6-bdf4-0800200c9a66`                                | `livi-runtime/src/bt.rs`                                      |
| iAP 服务 UUID     | `00000000-deca-fade-deca-deafdecacaff`                                | 同上                                                          |
| CarPlay 服务 UUID | `ec884348-cd41-40a2-9727-575d50bf1fd3`                                | 同上                                                          |
| 配置文件          | `~/.config/LIVI/config.json`                                          | `livi-core/src/paths.rs`                                      |
| MFi 目录          | `~/.config/LIVI/mfi/`                                                 | `livi-core/src/mfi.rs`                                        |
| 帧上限            | UI 16 MiB；host 协议 4 字节长度                                       | `core-proto/src/frame.rs`、`host-proto/src/lib.rs`            |
| 工作区版本        | `10.1.0`                                                              | `native/Cargo.toml`                                           |

## 附录 B：核心文件索引

| 关注点         | 文件                                                                                    |
| -------------- | --------------------------------------------------------------------------------------- |
| core 启动/退出 | `native/crates/livi-core/src/main.rs`                                                   |
| 进程编排       | `native/crates/livi-core/src/services.rs`                                               |
| UI 协议服务    | `native/crates/livi-core/src/server.rs`                                                 |
| 状态中枢       | `native/crates/livi-core/src/hub.rs`                                                    |
| 配置加载       | `native/crates/livi-core/src/config_file.rs`                                            |
| 投影会话       | `native/crates/livi-core/src/projection.rs` 及 `projection/`                            |
| CarPlay 集成   | `native/crates/livi-core/src/carplay.rs`                                                |
| AA 集成        | `native/crates/livi-core/src/android_auto.rs`                                           |
| 特权助手       | `native/crates/livi-core/src/helper.rs`、`native/crates/livi-helperd/src/linux_main.rs` |
| CarPlay 状态机 | `native/crates/livi-cp/src/stack.rs` 及 `stack/`                                        |
| AA 会话        | `native/crates/livi-aa-stack/src/manager.rs`、`channels/`                               |
| 视频解码选择   | `native/crates/livi-video/src/codec.rs`                                                 |
| 音频管线       | `native/crates/livi-audio/src/player.rs`                                                |
| 合成器         | `native/crates/livi-compositor/src/{drm,host,projection,ctrl}.rs`                       |
| GStreamer 宿主 | `native/crates/livi-gst-host/src/process.rs`、`gst.rs`                                  |
| UI 入口        | `native/crates/livi-ui/src/main.rs`、`ui/app.slint`                                     |
| 设置树         | `native/crates/livi-ui-model/src/settings/`                                             |
| 构建驱动       | `native/xtask/src/build_native.rs`、`package_linux.rs`、`package_mac.rs`                |
| 安装器         | `scripts/install/common.sh`、`headless/install.sh`                                      |
| GStreamer 打包 | `scripts/gstreamer/package-linux.sh`                                                    |
| 系统集成模板   | `assets/linux/*.template`                                                               |

## 附录 C：术语表

| 术语          | 含义                                                        |
| ------------- | ----------------------------------------------------------- |
| **iAP2**      | Apple 配件协议第 2 版，CarPlay 的会话/控制基础              |
| **MFi**       | Made for iPhone/iPod/iPad，Apple 配件认证体系；CarPlay 必需 |
| **NCM**       | USB 网络控制模型，iPhone 有线 CarPlay 的虚拟网卡            |
| **usbmux**    | Apple 的 USB 多路复用协议，用于访问 iOS 服务端口            |
| **carkit**    | iPhone 锁屏状态下可用的有线配件服务                         |
| **AOAP**      | Android Open Accessory Protocol，USB 配件切换协议           |
| **WPP**       | Wireless Projection Protocol，Android Auto 的帧封装         |
| **CSM**       | Control Session Message，iAP2 控制会话消息                  |
| **plane**     | 合成器/视频管线中的独立图层（主屏、仪表、摄像头等）         |
| **kiosk**     | 开机直接全屏启动 LIVI、无桌面环境的运行模式                 |
| **LIVI Link** | 项目自研的 CarPlay 无线适配器（自带 Wi-Fi/BT/MFi）          |
| **lifeline**  | 父进程通过 stdin 管道监视子进程存活的机制                   |

---

_本文基于代码静态分析编写。运行行为、硬件相关结论（无线性能、MFi、显示）请以板端实测为准；本板适配细节见 [RK3566-DEVELOPMENT.md](RK3566-DEVELOPMENT.md)。_
