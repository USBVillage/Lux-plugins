//! 魔改：把「插件自定义配置项」补进 Lux 自己的前端分片，让它能在网页里直接编辑。
//!
//! 背景：Lux 0.5.x 的前端 `assets/AdminPluginsPage.js` 对 `org.lux.strm-media-info`
//! 做了硬编码特判 —— 只渲染它认识的那 8 个键，manifest 里新加的 configFields
//! 既不在渲染里、也不在保存 payload 里。后端（API 层）是通用的，所以这是纯前端限制。
//!
//! 本模块在插件进程启动时检查该分片，如果还没打补丁就就地打上（幂等、带备份、
//! 锚点不唯一就跳过绝不乱改），并把结果写到 assets 目录下的状态文件里，
//! 这样可以直接用浏览器/curl 读取 `…/assets/lux-strm-ui-patch.status` 查看结果。
//!
//! 补丁内容由 `tools/patch_lux_ui.py --emit-rust` 生成，见 `lux_strm_ui_patch_data.rs`。

include!("lux_strm_ui_patch_data.rs");

use std::path::{Path, PathBuf};

/// 补丁状态文件（放在 web 目录里，便于用 HTTP 直接读）。
const STATUS_NAME: &str = "lux-strm-ui-patch.status";

/// 前端分片相对 web 根的路径。
const ASSET_REL: &str = "assets/AdminPluginsPage.js";

/// 入口：幂等打补丁。任何失败都只返回说明字符串，绝不影响插件本体功能。
pub fn ensure_patched() -> String {
    let Some(web_dir) = resolve_web_dir() else {
        return "SKIP 找不到 Lux web 目录（LUX_WEB_DIR 未设置且默认路径不存在）".to_owned();
    };

    let asset = web_dir.join(ASSET_REL);
    let source = match std::fs::read_to_string(&asset) {
        Ok(text) => text,
        Err(error) => {
            return report(
                &web_dir,
                &format!("SKIP 读不到 {}：{error}", asset.display()),
            );
        }
    };

    if source.contains(MARKER) {
        // 已经是补丁版，什么都不用做，也不需要反复写状态文件。
        return "OK 已是最新（marker 已存在）".to_owned();
    }

    for (index, (anchor, _)) in REPLACEMENTS.iter().enumerate() {
        let hits = source.matches(anchor).count();
        if hits != 1 {
            let preview: String = anchor.chars().take(48).collect();
            return report(
                &web_dir,
                &format!(
                    "SKIP 前端版本不匹配：第 {} 个锚点命中 {hits} 次（应为 1）：{preview}",
                    index + 1
                ),
            );
        }
    }

    let mut patched = source.clone();
    for (anchor, replacement) in REPLACEMENTS {
        patched = patched.replacen(anchor, replacement, 1);
    }

    if !patched.contains(MARKER) || patched.len() <= source.len() {
        return report(&web_dir, "SKIP 打补丁后自检失败，保持原样");
    }

    // 先把原文件留一份到 config 目录（不在 web 目录里，避免被当静态资源）。
    let backup_note = match backup_original(&source) {
        Some(path) => format!("，原文件已备份到 {}", path.display()),
        None => "（备份失败，仅内存中留有原文件）".to_owned(),
    };

    // 同目录写临时文件再 rename，避免 Lux 正在读时读到半个文件。
    let temp = asset.with_file_name("AdminPluginsPage.js.luxpatch");
    if let Err(error) = std::fs::write(&temp, patched.as_bytes()) {
        return report(
            &web_dir,
            &format!("FAIL 写临时文件失败（目录可能只读）：{error}"),
        );
    }
    if let Err(error) = std::fs::rename(&temp, &asset) {
        let _ = std::fs::remove_file(&temp);
        return report(
            &web_dir,
            &format!("FAIL 替换 {} 失败：{error}", asset.display()),
        );
    }

    report(
        &web_dir,
        &format!(
            "OK 已打补丁：{} 处替换，{} -> {} 字节{backup_note}；请硬刷新（Ctrl+F5）网页",
            REPLACEMENTS.len(),
            source.len(),
            patched.len()
        ),
    )
}

/// web 目录解析顺序：环境变量 → 官方镜像默认值 → 常见编译产物路径。
fn resolve_web_dir() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("LUX_WEB_DIR") {
        let dir = PathBuf::from(value);
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
    }
    for candidate in [
        "/usr/local/share/lux/web",
        "/app/web/dist",
        "/opt/lux/web",
        "/usr/share/lux/web",
    ] {
        let dir = PathBuf::from(candidate);
        if dir.join(ASSET_REL).is_file() {
            return Some(dir);
        }
    }
    None
}

/// 备份到 `{LUX_CONFIG_DIR}/web-ui-backup/`；已存在则不覆盖。
fn backup_original(source: &str) -> Option<PathBuf> {
    let config_dir = std::env::var_os("LUX_CONFIG_DIR")?;
    let dir = Path::new(&config_dir).join("web-ui-backup");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("AdminPluginsPage.original.js");
    if !path.exists() {
        std::fs::write(&path, source.as_bytes()).ok()?;
    }
    Some(path)
}

/// 把结果写到 web 目录和 config 目录，方便外部核对。
fn report(web_dir: &Path, message: &str) -> String {
    let line = format!("{message}\n");
    let assets = web_dir.join("assets");
    let _ = std::fs::write(assets.join(STATUS_NAME), line.as_bytes());
    if let Some(config_dir) = std::env::var_os("LUX_CONFIG_DIR") {
        let _ = std::fs::write(
            Path::new(&config_dir).join("lux-strm-ui-patch.status"),
            line.as_bytes(),
        );
    }
    message.to_owned()
}
