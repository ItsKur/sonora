use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Render, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowOptions, div, point, px, size,
};
use input::CloseWindow;
use state::{Playback, Queue, SideTab, Sonora};
use ui::ActiveTheme as _;

use crate::chrome::Aside;

const FIRST_SIZE: gpui::Size<gpui::Pixels> = size(px(420.), px(720.));
const LEAST_SIZE: gpui::Size<gpui::Pixels> = size(px(280.), px(320.));

/// A second window showing nothing but the lyrics panel, so a sheet can sit on another
/// display or beside another app while the main window is elsewhere.
pub struct LyricsPopout {
    aside: Entity<Aside>,
    focus: FocusHandle,
}

impl LyricsPopout {
    fn new(queue: Entity<Queue>, playback: Entity<Playback>, cx: &mut Context<Self>) -> Self {
        let aside = cx.new(|cx| Aside::new(queue, playback, SideTab::Lyrics, cx).popped());
        Self {
            aside,
            focus: cx.focus_handle(),
        }
    }
}

impl Render for LyricsPopout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();

        div()
            .track_focus(&self.focus)
            .key_context(input::WORKSPACE_CONTEXT)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.aside.clone())
    }
}

impl gpui::Focusable for LyricsPopout {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Remembers the popout so the action toggles one window rather than stacking them up.
#[derive(Default)]
struct Popout(Option<WindowHandle<LyricsPopout>>);

impl gpui::Global for Popout {}

/// Opens the lyrics popout, or closes it when it is already up.
pub fn toggle_lyrics_popout(cx: &mut App) {
    if let Some(open) = cx.default_global::<Popout>().0
        && open
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
    {
        cx.global_mut::<Popout>().0 = None;
        return;
    }

    let Sonora {
        playback, queue, ..
    } = Sonora::global(cx);
    let (playback, queue) = (playback.clone(), queue.clone());

    let settings = Sonora::global(cx).settings.read(cx);
    let look = settings.look();
    let background = ui::backdrop(look.blur, look.transparent);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let decorations = settings.window_decorations();

    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None, FIRST_SIZE, cx,
            ))),
            window_background: background,
            titlebar: Some(TitlebarOptions {
                title: Some(i18n::t!("lyrics-title")),
                appears_transparent: true,
                traffic_light_position: Some(point(px(9.), px(9.))),
            }),
            is_movable: true,
            is_resizable: true,
            app_id: Some("sonora".into()),
            window_min_size: Some(LEAST_SIZE),
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            window_decorations: Some(decorations),
            ..Default::default()
        },
        |window, cx| {
            window.set_rem_size(cx.theme().font_size);
            cx.new(|cx| LyricsPopout::new(queue, playback, cx))
        },
    );

    let handle = opened.expect("failed to open the lyrics window");
    handle
        .update(cx, |_, window, _| window.activate_window())
        .ok();
    cx.global_mut::<Popout>().0 = Some(handle);
}
