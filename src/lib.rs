//! Workspace anchor. 这里不放逻辑，只让根包存在（`cargo onedoc` 需要根包）。

/// 与 `Cargo.toml` 保持一致的版本号，便于 `cdc --version` 之外的场景取用。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
