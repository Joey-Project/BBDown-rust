[ [English](quick-start.md) | 简体中文 ]

# BBDown CLI 快速开始

## 1. 下载并运行

从 [GitHub Releases](https://github.com/Joey-Project/BBDown-rust/releases/latest) 下载对应系统和 CPU 架构的压缩包并解压。可在 `PATH` 中运行 `bbdown`。在解压目录运行时，macOS 和 Linux 使用 `./bbdown`，Windows PowerShell 使用 `.\bbdown.exe`。

<details>
<summary>FFmpeg 配置</summary>

BBDown 默认使用 FFmpeg 封装 MP4。[下载 FFmpeg](https://ffmpeg.org/download.html) 后，可将其加入 `PATH`，或用 `--ffmpeg` 指定路径，例如 `--ffmpeg ./tools/ffmpeg` 或 `--ffmpeg "C:\ffmpeg\bin\ffmpeg.exe"`。Windows 下也可将 `ffmpeg.exe` 放在 `bbdown.exe` 同一目录，并在该目录运行 BBDown。加上 `--no-mux` 可跳过封装并保留下载的媒体文件。

</details>

## 2. 可选的网页登录

需要会员权限或更高清晰度时，可以运行以下命令。

```bash
bbdown auth login-web
```

从 v0.11.3 起，终端空间足够时 BBDown 会直接显示二维码，设置 `NO_COLOR` 后也会显示。输出被重定向或终端空间不足时，可运行 `bbdown auth login-web --qr-output login.svg` 保存 SVG。用 B 站 App 扫码即可保存登录状态。v0.11.2 会打印 `scan:` 链接，可用本地工具将链接转成二维码。

## 3. 下载视频或分集

把示例替换成自己的 B 站链接或 ID。加 `--output-dir downloads` 可将文件保存到 `downloads` 目录。

| 下载内容 | 命令 |
| --- | --- |
| 普通视频（UGC） | `bbdown download BV1qt4y1X7TW` 或 `bbdown download av170001` |
| 番剧、影视、纪录片单集（PGC） | `bbdown download ep318903` |
| 整季 PGC | `bbdown download ss26801 --select all` |
| 指定几集 | `bbdown download ss26801 --select 1,3-5` |

`md...` 媒体页的 `--select` 在解析出的那一季内选集。`--select all` 可能包含预告等附加内容，可用 `bbdown info ss26801 --select all` 查看分集。

## 4. 清晰度、字幕和弹幕

- 运行 `bbdown plan av170001`，查看 `videos` 下可选的清晰度 ID，再给下载命令加 `--video-quality <ID>`。
- 可用字幕（包括 AI 字幕）默认另存。`--only subtitle` 只下载字幕，`--no-subtitles` 跳过字幕。`plan` 用 `ai=true` 标记识别到的 AI 字幕。`--subtitle-ai exclude-ai` 可排除 AI 字幕，`--subtitle-ai prefer-non-ai` 可优先选择同语言的人工字幕。
- XML 弹幕默认保存。`--danmaku-format xml,ass` 保存两种格式，`--only danmaku` 只下载弹幕，`--no-danmaku` 跳过弹幕。

更多选项请看[用户指南](user-guide.zh-CN.md)。
