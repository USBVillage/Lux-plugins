# Lux Plugins Mod

> **本仓库是 USBVillage 自建的 Lux 插件目录**（原仓库名 `Lux-plugins`，于 2026-10-03 更名为
> `Lux-plugins-mod`，改名后 GitHub 会保留旧地址的重定向）。
>
> - 内容 = Lux 官方插件目录 + **魔改版 `org.lux.strm-media-info`**
>   （额外支持「集中存放目录」「路径前缀剥离」「复用已有媒体信息」，并会在插件进程启动时自动给
>   Lux 前端分片 `assets/AdminPluginsPage.js` 打补丁，让这三项真正显示在配置弹窗里）。
> - **与 `USBVillage/Lux-plugins-plus` 是两个互相独立的项目**，发布时不要互相覆盖。
>   后者是 TMDb 元数据增强（`org.lux.tmdb-plus`）的独立目录。
> - Lux 的「插件商店地址」同时只能填一个，见下方「插件商店地址」一节。
>
> 本仓库的 GitHub Actions 已被关闭，发布走本地 `tools/publish_to_github.py`（手动生成
> `index.json` + 上传 Release 资产）。

This repository is the default plugin store for [Lux](https://github.com/Qoo-330ml/Lux).

This repository contains the plugin source code. A push to `main` starts
`.github/workflows/release.yml`, which compares `plugins.json` with the previous revision and only
packages and publishes plugins that were added or whose version changed. Bump a plugin's catalog
version when its source or manifest should be published; source-only merges leave existing Releases
and `index.json` untouched. The workflow builds on both `linux-x86_64` and `linux-aarch64` matching
GitHub-hosted runners and merges changed plugin entries into `index.json`, preserving unchanged
entries. Each plugin owns a stable Release whose tag is its plugin ID; versioned assets are
append-only. Re-running a release with identical indexed package bytes is a no-op, while different
bytes for an already-published asset fail and require a version bump instead of overwriting it.
Removing a plugin ID drops it from the active catalog but does not delete its historical Release or
assets. Lux validates the ZIP and its manifest before installing it into `/config/plugins`.

The package asset name includes the plugin version and target architecture, for example
`org.lux.tmdb-0.1.12-linux-x86_64.zip` and `org.lux.tmdb-0.1.12-linux-aarch64.zip`. The Lux host
selects the matching package from `packages` and stores it under its own canonical plugin ZIP
name after downloading it.

Do not commit credentials, local configuration, media data, or unreviewed executable packages.

## Emby 迁移助手

`org.lux.emby-migration` implements the one-way Lux migration contract. It connects to an administrator-approved
Emby base URL using a request-scoped API key, returns bounded user, item-state, and user-level Person favorite pages, and performs one-time
user-password verification for accounts created by Lux. It never reads an Emby database, returns an Emby access token,
or implements reverse migration. The current plugin reports `ITEM_STATE`; it does not synthesize a playback history
timeline from aggregate UserData. When the host supplies `supportsFilteredReads` projections, it limits user IDs, user fields,
state fields, and source library IDs before issuing Emby requests; legacy requests retain their previous complete-read behavior.

## Douban metadata

`org.lux.douban` (provider key `douban`) implements the Lux v1 metadata RPC contract for Douban. It supports Movie and
Series search, metadata bundles, poster images, cast/director credits, the Douban provider ID,
and available trailers. Season metadata is supported when the upstream subject represents a
season; episode, person, and collection metadata are reported as unsupported because the
referenced Douban mobile API does not expose a stable equivalent.

Search uses Douban's public subject-suggest endpoint. Details and richer metadata use the
WeChat-compatible client with the public client credential shipped by the upstream Jellyfin
Douban plugin. No credential configuration is required; the plugin is usable immediately after
installation. The optional `requestIntervalMs` setting only tunes request pacing. For private
testing or a future credential rotation, environment variables can override the built-in client
key without changing the package. Credentials are never included in RPC results or logs. The
plugin applies a bounded response size, HTTPS endpoint validation, rate limiting, retries for
timeouts/429/5xx, and a short-lived bounded response cache. Setting `LUX_DOUBAN_API_BASE_URL` to
the legacy `https://api.douban.com/v2/` endpoint selects the request shape used by the inspected
Emby DLL; the default remains the currently supported WeChat-compatible endpoint.

## Intro/outro detector

`org.lux.intro-outro-detector` implements the Lux v1 `chapter_detector` contract. It receives only
bounded raw Chromaprint point sequences selected by Lux for at least two episodes in one season.
Its manifest declares `supportedMediaSourceKinds: ["LOCAL_FILE"]`; this declaration controls which
host media sources become candidates and does not expose paths to the plugin.
Each Base64 payload is a little-endian sequence of `uint32` fingerprint points; one point represents
`1,238,095` ticks. The detector compares aligned points with a bounded Hamming-distance tolerance,
requires a non-trivial shared sequence, and uses support across the available episodes before
emitting a candidate. It does not invoke ffmpeg, access media paths, open network connections, or
receive source IDs and URLs. It returns only `IntroStart`, `IntroEnd`, and `CreditsStart` candidates;
Lux remains responsible for time-range validation, confidence filtering, persistence, and
Emby-compatible chapter output.

## TheIntroDB online chapter source

`org.lux.theintrodb-chapter-source` is an independent online chapter source. It queries
[TheIntroDB](https://theintrodb.org/) using stored TMDb, TVDb, or IMDb metadata, season/episode numbers,
and optional runtime. It receives no media path, URL, audio fingerprint, or task object, and never runs
ffmpeg or ffprobe. Its manifest declares `supportedMediaSourceKinds: ["LOCAL_FILE", "STRM_URL"]`;
the host uses that declaration to include local and `.strm` entries without sending either path or URL.
Empty upstream results preserve existing chapters. Its exact boundary and configuration
are documented in `README-theintrodb.md`.

## Webhook 通知器

`org.lux.webhook` implements the Lux v1 `notification.send` contract. Lux supplies the
provider-neutral event, including the unified `source`, `title`, `content`, `body`, and
`timestamp` fields; the plugin validates the destination again, resolves all DNS addresses,
blocks redirects and sends an HMAC-SHA256 signed JSON request. Its `payloadFormat` setting only
selects the outer Lux-native or limited Emby-style transport shape; it does not generate or
rewrite notification text. The target URL may reference Lux-generated fields with URL encoding.
Delivery queues, retry scheduling and secret storage remain owned by Lux, so this plugin has no
access to the Lux configuration directory or database.

## 插件商店地址（重要）

Lux **只支持一个**商店地址，存在 `{LUX_CONFIG_DIR}/plugin_store_url` 单个文件里，同时在
管理界面「插件商店」页可以改（`GET/PUT /api/v1/admin/plugin-store`）。

地址格式规则（源码 `src/application/plugin_store.rs::catalog_url`）：

- `https://github.com/<owner>/<repo>`（**正好两段路径**）→ 自动改写为
  `https://raw.githubusercontent.com/<owner>/<repo>/main/index.json`。**这是推荐写法。**
- 带多余路径（例如 `/releases/tag/xxx`）或非 GitHub 的地址 → **原样去拉**，拉回来必须是
  Lux 目录 JSON，否则解析失败、商店目录为空（表现为插件的 `latestVersion` 为 `null`、
  永不提示更新）。所以**不要**把 GitHub Release 页面地址填进去。

| 用途 | 商店地址 |
| --- | --- |
| 本仓库（官方插件 + 魔改 strm） | `https://github.com/USBVillage/Lux-plugins-mod` |
| TMDb 增强（独立项目） | `https://github.com/USBVillage/Lux-plugins-plus` |
| Lux 官方默认 | `https://github.com/Qoo-330ml/Lux-plugins` |

两个自建仓库**不能同时生效**——`Lux-plugins-mod` 里已包含官方全部插件，日常挂在它上面即可；
要更新 `tmdb-plus` 时把商店地址临时切到 `Lux-plugins-plus`，装完再切回来。
切换只是改配置，不会卸载已装插件，也不会丢插件配置。

