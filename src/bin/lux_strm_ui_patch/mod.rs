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
//! 持久化：打补丁成功后会把「补丁版 + 原版」副本写进 `{LUX_CONFIG_DIR}/web-patch/`。
//! web 目录在容器层里，容器重建就还原；把下面这行加进 compose 的 `command` 即可
//! 在每次容器启动时自动恢复（版本校验失败会自动跳过，升级 Lux 也不会错配）：
//!   sh -c 'cmp -s /usr/local/share/lux/web/assets/AdminPluginsPage.js
//!          /config/web-patch/AdminPluginsPage.stock.js
//!          && cp /config/web-patch/AdminPluginsPage.patched.js
//!          /usr/local/share/lux/web/assets/AdminPluginsPage.js; exec /usr/local/bin/luxd'
//!
//! 补丁内容由 `tools/patch_lux_ui.py --emit-rust` 生成，见 `lux_strm_ui_patch_data.rs`。

include!("lux_strm_ui_patch_data.rs");

use std::path::{Path, PathBuf};

/// 补丁状态文件（放在 web 目录里，便于用 HTTP 直接读）。
const STATUS_NAME: &str = "lux-strm-ui-patch.status";

/// 前端分片相对 web 根的路径。
const ASSET_REL: &str = "assets/AdminPluginsPage.js";

/// 持久化副本目录（config 卷，容器重建不丢）。配合容器启动脚本：
/// 先 cmp 校验 Lux 版本未变（当前 web 文件 == stock 副本），再把 patched 副本
/// 覆盖回 web 目录。升级 Lux 后校验自然失败，绝不把旧前端套到新后端上。
const WEB_PATCH_DIR: &str = "web-patch";
/// 持久化的补丁版分片。
const PATCHED_NAME: &str = "AdminPluginsPage.patched.js";
/// 持久化的原版分片（打补丁那一刻的 web 文件，用于版本校验）。
const STOCK_NAME: &str = "AdminPluginsPage.stock.js";

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
        // 已经是补丁版。这里仍然过一遍 report，用来把上一次并发竞争留下的
        // 误导性状态（例如 "FAIL ... No such file"）自我纠正。
        let note = ensure_persisted_from_existing(&source);
        return report(
            &web_dir,
            &format!("OK 已是最新（marker 已存在，无需处理）{note}"),
        );
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

    // 关键：临时文件名必须带上 pid。
    // Lux 是按单个探测目标起独立进程的（concurrency=2 时会有两个插件进程同时启动），
    // 如果共用一个临时文件名，A 进程 rename 掉的可能是 B 进程正在写一半的文件，
    // 从而把半截 JS 换到线上。用 pid 隔离后再 rename，才真正原子。
    let temp = asset.with_file_name(format!("AdminPluginsPage.js.luxpatch.{}", std::process::id()));
    if let Err(error) = std::fs::write(&temp, patched.as_bytes()) {
        return report(
            &web_dir,
            &format!("FAIL 写临时文件失败（web 目录可能只读）：{error}"),
        );
    }
    if let Err(error) = std::fs::rename(&temp, &asset) {
        let _ = std::fs::remove_file(&temp);
        // 竞争兜底：可能另一个进程已经补好了。
        let raced = std::fs::read_to_string(&asset).ok().filter(|text| text.contains(MARKER));
        if let Some(current) = raced {
            let note = ensure_persisted_from_existing(&current);
            return report(
                &web_dir,
                &format!("OK 已是最新（另一个插件进程已完成补丁）{note}"),
            );
        }
        return report(
            &web_dir,
            &format!("FAIL 替换 {} 失败：{error}", asset.display()),
        );
    }

    let persist_note = persist_copies(&source, &patched);
    report(
        &web_dir,
        &format!(
            "OK 已打补丁：{} 处替换，{} -> {} 字节{backup_note}{persist_note}；请硬刷新（Ctrl+F5）网页",
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

/// 打补丁成功后把「补丁版 + 原版」副本写进 config 卷（容器重建不丢），
/// 供容器启动脚本恢复补丁。写入失败只影响自动恢复，不影响本次补丁。
fn persist_copies(source: &str, patched: &str) -> String {
    let Some(dir) = web_patch_dir() else {
        return String::new();
    };
    let _ = std::fs::create_dir_all(&dir);
    let patched_ok = std::fs::write(dir.join(PATCHED_NAME), patched.as_bytes()).is_ok();
    let stock_ok = std::fs::write(dir.join(STOCK_NAME), source.as_bytes()).is_ok();
    match (patched_ok, stock_ok) {
        (true, true) => "；已持久化补丁副本到 config/web-patch/（配合容器启动自动恢复）".to_owned(),
        _ => "；持久化补丁副本写入失败".to_owned(),
    }
}

/// web 文件已经是补丁版但持久化副本缺失时（例如升级插件前就打过补丁），
/// 用当前 web 文件和 web-ui-backup 里的原版重建副本。
fn ensure_persisted_from_existing(patched_source: &str) -> String {
    let Some(dir) = web_patch_dir() else {
        return String::new();
    };
    if dir.join(PATCHED_NAME).is_file() && dir.join(STOCK_NAME).is_file() {
        return String::new();
    }
    let stock = dir
        .parent()
        .map(|config_dir| config_dir.join("web-ui-backup").join("AdminPluginsPage.original.js"));
    let stock_source = match stock {
        Some(path) => std::fs::read_to_string(path).ok().filter(|text| !text.contains(MARKER)),
        None => None,
    };
    let Some(stock_source) = stock_source else {
        return "；持久化副本不完整（缺干净的原版备份）".to_owned();
    };
    let _ = std::fs::create_dir_all(&dir);
    let patched_ok = std::fs::write(dir.join(PATCHED_NAME), patched_source.as_bytes()).is_ok();
    let stock_ok = std::fs::write(dir.join(STOCK_NAME), stock_source.as_bytes()).is_ok();
    match (patched_ok, stock_ok) {
        (true, true) => "；持久化副本已生成到 config/web-patch/".to_owned(),
        _ => "；持久化副本写入失败".to_owned(),
    }
}

fn web_patch_dir() -> Option<PathBuf> {
    std::env::var_os("LUX_CONFIG_DIR").map(|config_dir| Path::new(&config_dir).join(WEB_PATCH_DIR))
}

/// 把结果写到 web 目录和 config 目录，方便外部核对（内容一致就不重复落盘）。
fn report(web_dir: &Path, message: &str) -> String {
    let line = format!("{message}\n");
    write_if_changed(&web_dir.join("assets").join(STATUS_NAME), &line);
    if let Some(config_dir) = std::env::var_os("LUX_CONFIG_DIR") {
        write_if_changed(
            &Path::new(&config_dir).join("lux-strm-ui-patch.status"),
            &line,
        );
    }
    message.to_owned()
}

fn write_if_changed(path: &Path, content: &str) {
    if std::fs::read_to_string(path).is_ok_and(|current| current == content) {
        return;
    }
    let _ = std::fs::write(path, content.as_bytes());
}
