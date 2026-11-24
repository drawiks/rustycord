use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    ExtraIcons,
    [
        AtSign,
        Hash,
        LogOut,
        MessageSquare,
        Pencil,
        Reply,
        Send,
        Trash,
        X
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Ok(Assets.load(path).ok().flatten())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ExtraIcons.list(path)?;
        paths.extend(Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_extra_and_default_icons() {
        let assets = AppAssets;
        assert!(assets.load("icons/reply.svg").unwrap().is_some());
        assert!(assets.load("icons/hash.svg").unwrap().is_some());
        assert!(assets.load("icons/chevron-down.svg").unwrap().is_some());
        assert!(assets.load("icons/check.svg").unwrap().is_some());
    }
}
