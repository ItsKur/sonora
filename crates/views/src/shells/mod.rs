pub(crate) mod fullscreen;
pub mod popout;
pub(crate) mod workspace;

use gpui::{AnyView, App};

use crate::chrome::TitleBarOptions;

pub(crate) trait Shell {
    fn title_bar(&self, content: Option<AnyView>, cx: &App) -> TitleBarOptions;
}
