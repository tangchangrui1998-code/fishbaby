use crate::{changed, persist, Store};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub display: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, PartialEq)]
struct Screen {
    key: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}
fn clamp(layout: &Layout, screen: &Screen) -> Layout {
    let width = layout.width.clamp(220., screen.width.max(220.));
    let height = layout.height.clamp(90., screen.height.max(90.));
    Layout {
        display: screen.key.clone(),
        x: layout.x.clamp(0., (screen.width - width).max(0.)),
        y: layout.y.clamp(0., (screen.height - height).max(0.)),
        width,
        height,
    }
}
pub struct Tracker {
    checked: Instant,
    screens: Vec<Screen>,
    previous: Option<(Screen, Layout)>,
}
impl Tracker {
    pub fn new() -> Self {
        Self {
            checked: Instant::now() - Duration::from_secs(2),
            screens: vec![],
            previous: None,
        }
    }
    pub fn update(&mut self, window: &tauri::WebviewWindow, store: &Store) -> tauri::Result<()> {
        if self.checked.elapsed() < Duration::from_millis(750) {
            return Ok(());
        }
        self.checked = Instant::now();
        let monitors = window.available_monitors()?;
        let screens: Vec<_> = monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let area = m.work_area();
                let scale = m.scale_factor();
                Screen {
                    key: m
                        .name()
                        .cloned()
                        .unwrap_or_else(|| format!("显示器 {}", i + 1)),
                    x: area.position.x as f64 / scale,
                    y: area.position.y as f64 / scale,
                    width: area.size.width as f64 / scale,
                    height: area.size.height as f64 / scale,
                    scale,
                }
            })
            .collect();
        if screens.is_empty() {
            return Ok(());
        }
        let current = window
            .current_monitor()?
            .and_then(|m| screens.iter().find(|s| Some(&s.key) == m.name()).cloned())
            .unwrap_or_else(|| screens[0].clone());
        let position = window.outer_position()?;
        let scale = window.scale_factor()?;
        let editable = matches!(store.mode.lock().unwrap().name.as_str(), "ready" | "setup");
        let mut data = store.library.lock().unwrap();
        let observed = Layout {
            display: current.key.clone(),
            x: position.x as f64 / scale - current.x,
            y: position.y as f64 / scale - current.y,
            width: data.settings.width,
            height: data.settings.height,
        };
        let transitioned = self
            .previous
            .as_ref()
            .is_some_and(|(old, _)| old.key != current.key || self.screens != screens);
        let initial = self.previous.is_none();
        let target = if transitioned || initial {
            if let Some((_, previous)) = &self.previous {
                data.tools
                    .layouts
                    .retain(|layout| layout.display != previous.display);
                data.tools.layouts.push(previous.clone());
            }
            let preferred = data
                .tools
                .layouts
                .iter()
                .find(|l| l.display == current.key)
                .cloned()
                .unwrap_or(observed.clone());
            clamp(&preferred, &current)
        } else if editable {
            observed.clone()
        } else {
            // Background window-management moves must not overwrite the last
            // intentional layout for this display.
            data.tools
                .layouts
                .iter()
                .find(|l| l.display == current.key)
                .cloned()
                .unwrap_or(observed.clone())
        };
        let target = clamp(&target, &current);
        let needs_move = transitioned
            || (initial
                && (target.x != observed.x
                    || target.y != observed.y
                    || target.width != observed.width
                    || target.height != observed.height));
        if needs_move {
            data.settings.width = target.width;
            data.settings.height = target.height;
            data.settings.x = Some(current.x + target.x);
            data.settings.y = Some(current.y + target.y);
            let _ = persist(store, &data);
        }
        if !needs_move {
            let bounded = clamp(&target, &current);
            let old = data.tools.layouts.iter().find(|l| l.display == current.key);
            if old.is_none_or(|old| {
                old.x != bounded.x
                    || old.y != bounded.y
                    || old.width != bounded.width
                    || old.height != bounded.height
            }) {
                data.tools.layouts.retain(|l| l.display != current.key);
                data.tools.layouts.push(bounded);
                if data.tools.layouts.len() > 16 {
                    data.tools.layouts.remove(0);
                }
                let _ = persist(store, &data);
            }
        }
        drop(data);
        if needs_move {
            window.set_position(tauri::LogicalPosition::new(
                current.x + target.x,
                current.y + target.y,
            ))?;
            let mut session = store.mode.lock().unwrap();
            session.pointer_inside = None;
            session.entered_at = None;
            drop(session);
            store
                .geometry_generation
                .store(u64::MAX, std::sync::atomic::Ordering::Release);
            crate::diagnostics::record(
                "display-layout-restored",
                serde_json::json!({"width":target.width,"height":target.height}),
            );
            changed(window.app_handle());
        }
        self.previous = Some((current, target));
        self.screens = screens;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unplugged_or_smaller_display_keeps_layout_inside_work_area() {
        let screen = Screen {
            key: "laptop".into(),
            x: 0.,
            y: 0.,
            width: 1280.,
            height: 720.,
            scale: 2.,
        };
        let old = Layout {
            display: "external".into(),
            x: 2000.,
            y: -300.,
            width: 1800.,
            height: 900.,
        };
        let next = clamp(&old, &screen);
        assert_eq!(
            (next.x, next.y, next.width, next.height),
            (0., 0., 1280., 720.)
        );
        assert_eq!(old.display, "external");
    }
}
