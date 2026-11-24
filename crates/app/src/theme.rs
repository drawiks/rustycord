pub const RAIL: u32 = 0x1e1f22;
pub const SIDEBAR: u32 = 0x2b2d31;
pub const CHAT: u32 = 0x313338;
pub const HOVER: u32 = 0x3f4147;
pub const ACTIVE: u32 = 0x404249;
pub const TEXT: u32 = 0xdbdee1;
pub const MUTED: u32 = 0x949ba4;
pub const FAINT: u32 = 0x6d6f78;
pub const ACCENT: u32 = 0x5865f2;
pub const DANGER: u32 = 0xed4245;
pub const CODE_TEXT: u32 = 0xdcddde;
pub const CODE_BG: u32 = 0x2b2d31;
pub const LINK: u32 = 0x00a8fc;
pub const RAIL_HOVER: u32 = 0x35373c;
pub const TEXT_BRIGHT: u32 = 0xf2f3f5;
pub const ICON: u32 = 0x80848e;
pub const ONLINE: u32 = 0x23a55a;
pub const IDLE: u32 = 0xf0b232;
pub const DND: u32 = 0xf23f43;
pub const PANEL: u32 = 0x232428;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb};

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = "Inter Variable".into();
        theme.mono_font_family = "JetBrains Mono".into();
        theme.font_size = px(14.);
        theme.mono_font_size = px(13.);
        theme.radius = px(6.);
        theme.radius_lg = px(12.);
        theme.shadow = true;

        theme.colors.background = c(CHAT);
        theme.colors.foreground = c(TEXT);
        theme.colors.border = c(RAIL);
        theme.colors.input = c(0x383a40);
        theme.colors.caret = c(0xffffff);
        theme.colors.ring = c(ACCENT);
        theme.colors.selection = c(ACCENT).opacity(0.3);
        theme.colors.muted = c(SIDEBAR);
        theme.colors.muted_foreground = c(MUTED);
        theme.colors.popover = c(SIDEBAR);
        theme.colors.popover_foreground = c(TEXT);

        theme.colors.primary = c(ACCENT);
        theme.colors.primary_hover = c(0x4752c4);
        theme.colors.primary_active = c(0x4752c4);
        theme.colors.primary_foreground = c(0xffffff);
        theme.colors.secondary = c(0x4e5058);
        theme.colors.secondary_hover = c(0x5d6068);
        theme.colors.secondary_active = c(0x6d6f78);
        theme.colors.secondary_foreground = c(0xffffff);
        theme.colors.accent = c(0x404249);
        theme.colors.accent_foreground = c(TEXT_BRIGHT);

        theme.colors.button = c(0x4e5058);
        theme.colors.button_hover = c(0x5d6068);
        theme.colors.button_active = c(0x6d6f78);
        theme.colors.button_foreground = c(0xffffff);
        theme.colors.button_primary = c(ACCENT);
        theme.colors.button_primary_hover = c(0x4752c4);
        theme.colors.button_primary_active = c(0x4752c4);
        theme.colors.button_primary_foreground = c(0xffffff);
        theme.colors.button_secondary = c(0x4e5058);
        theme.colors.button_secondary_hover = c(0x5d6068);
        theme.colors.button_secondary_active = c(0x6d6f78);
        theme.colors.button_secondary_foreground = c(0xffffff);
        theme.colors.button_danger = c(DANGER);
        theme.colors.button_danger_hover = c(0xa1282d);
        theme.colors.button_danger_active = c(0xa1282d);
        theme.colors.button_danger_foreground = c(0xffffff);

        theme.colors.danger = c(DANGER);
        theme.colors.danger_hover = c(0xa1282d);
        theme.colors.danger_active = c(0xa1282d);
        theme.colors.danger_foreground = c(0xffffff);
        theme.colors.success = c(ONLINE);
        theme.colors.success_foreground = c(0xffffff);
        theme.colors.warning = c(IDLE);
        theme.colors.warning_foreground = c(RAIL);
        theme.colors.info = c(ACCENT);
        theme.colors.info_foreground = c(0xffffff);
        theme.colors.link = c(LINK);
        theme.colors.link_hover = c(LINK);
        theme.colors.link_active = c(LINK);

        theme.colors.list = c(CHAT);
        theme.colors.list_even = c(SIDEBAR);
        theme.colors.list_head = c(RAIL);
        theme.colors.list_hover = c(RAIL_HOVER);
        theme.colors.list_active = c(ACTIVE);
        theme.colors.list_active_border = c(ACCENT);
        theme.colors.list_highlight_foreground = c(0xffffff);

        theme.colors.scrollbar = c(SIDEBAR);
        theme.colors.scrollbar_thumb = c(0x1a1b1e);
        theme.colors.scrollbar_thumb_hover = c(0x111214);

        theme.colors.sidebar = c(SIDEBAR);
        theme.colors.sidebar_foreground = c(TEXT);
        theme.colors.sidebar_border = c(RAIL);
        theme.colors.sidebar_accent = c(ACTIVE);
        theme.colors.sidebar_accent_foreground = c(0xffffff);
        theme.colors.sidebar_primary = c(ACCENT);
        theme.colors.sidebar_primary_foreground = c(0xffffff);

        theme.colors.tab_bar = c(RAIL);
        theme.colors.tab_bar_segmented = c(PANEL);
        theme.colors.tab = c(SIDEBAR);
        theme.colors.tab_active = c(ACTIVE);
        theme.colors.tab_active_foreground = c(0xffffff);
        theme.colors.tab_foreground = c(MUTED);

        theme.colors.table = c(CHAT);
        theme.colors.table_even = c(SIDEBAR);
        theme.colors.table_head = c(RAIL);
        theme.colors.table_head_foreground = c(MUTED);
        theme.colors.table_hover = c(RAIL_HOVER);
        theme.colors.table_active = c(ACTIVE);
        theme.colors.table_active_border = c(ACCENT);
        theme.colors.table_row_border = c(RAIL);
        theme.colors.table_foot = c(RAIL);
        theme.colors.table_foot_foreground = c(MUTED);

        theme.colors.title_bar = c(RAIL);
        theme.colors.title_bar_border = c(RAIL);
        theme.colors.status_bar = c(PANEL);
        theme.colors.status_bar_border = c(RAIL);
        theme.colors.window_border = c(RAIL);
        theme.colors.overlay = c(0x000000).opacity(0.7);
        theme.colors.drag_border = c(ACCENT);
        theme.colors.drop_target = c(0x3c4270);
        theme.colors.progress_bar = c(ACCENT);
        theme.colors.switch = c(ACCENT);
        theme.colors.switch_thumb = c(0xffffff);
        theme.colors.skeleton = c(RAIL_HOVER);
        theme.colors.slider_bar = c(ACCENT);
        theme.colors.slider_thumb = c(0xffffff);
        theme.colors.group_box = c(SIDEBAR);
        theme.colors.group_box_foreground = c(TEXT);
        theme.colors.accordion = c(SIDEBAR);
        theme.colors.description_list_label = c(SIDEBAR);
        theme.colors.description_list_label_foreground = c(MUTED);
    });
}
