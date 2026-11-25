use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    ExtraIcons,
    [
        AtSign,
        Bell,
        Compass,
        Crown,
        Download,
        Gift,
        Hash,
        Headphones,
        LogOut,
        Megaphone,
        MessageSquare,
        Paperclip,
        Pencil,
        Pin,
        Reply,
        Send,
        Sticker,
        Trash,
        UserRound,
        Users,
        Volume2,
        X
    ]
);

const CUSTOM_ICONS: &[(&str, &[u8])] = &[(
    "icons/smile.svg",
    include_bytes!("../assets/icons/smile.svg"),
)];

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = CUSTOM_ICONS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Ok(Assets.load(path).ok().flatten())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> = CUSTOM_ICONS
            .iter()
            .map(|(name, _)| SharedString::from(*name))
            .filter(|name| name.as_ref().starts_with(path))
            .collect();
        paths.extend(ExtraIcons.list(path)?);
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
    fn serves_extra_default_and_custom_icons() {
        let assets = AppAssets;
        assert!(assets.load("icons/reply.svg").unwrap().is_some());
        assert!(assets.load("icons/users.svg").unwrap().is_some());
        assert!(assets.load("icons/chevron-down.svg").unwrap().is_some());
        assert!(assets.load("icons/smile.svg").unwrap().is_some());
    }
}
