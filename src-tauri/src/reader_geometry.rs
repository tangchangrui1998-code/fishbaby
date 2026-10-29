//! Preserve logical reading dimensions across display scaling and system window rearrangements.
use super::{changed, diagnostics, persist, Store};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::Manager;
#[cfg(windows)]
mod windows;

fn editable(mode: &str) -> bool {
    matches!(mode, "ready" | "setup")
}

fn logical_size(width: u32, height: u32, scale: f64) -> Option<(f64, f64)> {
    (width > 0 && height > 0 && scale.is_finite() && scale > 0.)
        .then(|| (width as f64 / scale, height as f64 / scale))
}

fn differs(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() > 1. || (a.1 - b.1).abs() > 1.
}

trait GeometryWindow {
    fn configure(&self, locked: bool, size: (f64, f64)) -> tauri::Result<()>;
    fn sample(&self) -> tauri::Result<(u32, u32, f64)>;
    fn resize(&self, size: (f64, f64)) -> tauri::Result<()>;
}
impl GeometryWindow for tauri::WebviewWindow {
    fn configure(&self, locked: bool, size: (f64, f64)) -> tauri::Result<()> {
        #[cfg(windows)]
        return windows::configure(self.hwnd()?.0, locked).map_err(Into::into);
        #[cfg(not(windows))]
        {
            self.set_size_constraints(tauri::WindowSizeConstraints {
                min_width: Some(tauri::LogicalUnit(if locked { size.0 } else { 220. }).into()),
                min_height: Some(tauri::LogicalUnit(if locked { size.1 } else { 90. }).into()),
                max_width: locked.then(|| tauri::LogicalUnit(size.0).into()),
                max_height: locked.then(|| tauri::LogicalUnit(size.1).into()),
                ..Default::default()
            })?;
            self.set_resizable(!locked)?;
            self.set_maximizable(false)
        }
    }
    fn sample(&self) -> tauri::Result<(u32, u32, f64)> {
        let scale = self.scale_factor()?;
        let size = self.inner_size()?;
        Ok((size.width, size.height, scale))
    }
    fn resize(&self, size: (f64, f64)) -> tauri::Result<()> {
        #[cfg(windows)]
        {
            let scale = self.scale_factor()?;
            windows::resize(
                self.hwnd()?.0,
                (size.0 * scale).round() as i32,
                (size.1 * scale).round() as i32,
            )
            .map_err(Into::into)
        }
        #[cfg(not(windows))]
        self.set_size(tauri::LogicalSize::new(size.0, size.1))
    }
}

pub fn resize_reader(window: &tauri::WebviewWindow, size: (f64, f64)) -> tauri::Result<()> {
    window.resize(size)
}

/// Owned by the polling worker. Never hold application locks across native calls.
pub struct Protection {
    configuration: Option<(bool, (f64, f64), u64)>,
    checked: Instant,
    pending_restore: bool,
}
impl Protection {
    pub fn new() -> Self {
        Self {
            configuration: None,
            checked: Instant::now(),
            pending_restore: false,
        }
    }

    pub fn reconcile(
        &mut self,
        window: &tauri::WebviewWindow,
        store: &Store,
    ) -> tauri::Result<bool> {
        let (mode, generation) = {
            let session = store.mode.lock().unwrap();
            (session.name.clone(), session.generation)
        };
        let expected = {
            let data = store.library.lock().unwrap();
            (data.settings.width, data.settings.height)
        };
        if store.geometry_generation.load(Ordering::Acquire) != generation && !self.pending_restore
        {
            self.configuration = None;
        }
        if self.needs_configuration(!editable(&mode), expected, generation) || self.pending_restore
        {
            store.geometry_generation.store(u64::MAX, Ordering::Release);
        }
        let ready = self.apply(window, &mode, generation, expected)?;
        store
            .geometry_generation
            .store(if ready { generation } else { u64::MAX }, Ordering::Release);
        Ok(ready)
    }

    fn needs_configuration(&self, locked: bool, expected: (f64, f64), generation: u64) -> bool {
        self.configuration.is_none_or(|previous| {
            previous.0 != locked || previous.2 != generation || (locked && previous.1 != expected)
        })
    }

    fn apply(
        &mut self,
        window: &impl GeometryWindow,
        mode: &str,
        generation: u64,
        expected: (f64, f64),
    ) -> tauri::Result<bool> {
        let locked = !editable(mode);
        let next = (locked, expected, generation);
        let reconfigure = self.needs_configuration(locked, expected, generation);
        if !reconfigure
            && !self.pending_restore
            && self.checked.elapsed() < Duration::from_millis(250)
        {
            return Ok(true);
        }
        self.checked = Instant::now();
        if reconfigure {
            // Hidden readers stay mounted for hover detection. Disable native
            // resizing while reading, and repair system-driven size changes.
            window.configure(locked, expected)?;
            diagnostics::record(
                "geometry-policy",
                json!({
                    "mode": mode, "generation": generation, "locked": locked, "logicalSize": expected,
                }),
            );
        }
        #[cfg(windows)]
        if locked && !reconfigure {
            // Other Tauri window operations can reapply its cached style flags.
            // Reassert native sizing without activation or z-order changes.
            window.configure(locked, expected)?;
        }
        let (width, height, scale) = window.sample()?;
        let actual = logical_size(width, height, scale);
        // Restore before showing a preview as well as while hidden/reading.
        // Preview edits remain free once its mode/size configuration is applied.
        if actual.is_some_and(|actual| differs(actual, expected))
            && (locked || reconfigure || self.pending_restore)
        {
            diagnostics::record(
                "geometry-restore",
                json!({
                    "mode": mode, "actualLogicalSize": actual, "expectedLogicalSize": expected,
                    "physicalSize": [width, height], "scale": scale,
                }),
            );
            window.resize(expected)?;
            self.pending_restore = true;
            self.configuration = Some(next);
            // Native sizing can be asynchronous. Confirm on the next poll before
            // displaying a reader or accepting resize preferences.
            return Ok(false);
        }
        self.pending_restore = false;
        self.configuration = Some(next);
        Ok(true)
    }
}

pub fn on_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    let kind = match event {
        tauri::WindowEvent::Moved(_) => "move",
        tauri::WindowEvent::Resized(_) => "resize",
        tauri::WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => {
            if let Some(store) = window.try_state::<Store>() {
                store.geometry_generation.store(u64::MAX, Ordering::Release);
            }
            diagnostics::record(
                "display-scale-event",
                json!({
                    "scale": scale_factor, "physicalSize": [new_inner_size.width, new_inner_size.height],
                }),
            );
            // This is a system transformation, never a manual size preference.
            return;
        }
        _ => return,
    };
    let Some(store) = window.try_state::<Store>() else {
        return;
    };
    let (was_editable, generation) = {
        let session = store.mode.lock().unwrap();
        (
            editable(&session.name)
                && store.geometry_generation.load(Ordering::Acquire) == session.generation,
            session.generation,
        )
    };
    let window = window.clone();
    // Read live size and scale together after the native callback. Pairing an
    // old Resized payload with a new display scale can double a saved width.
    tauri::async_runtime::spawn(async move {
        let store = window.state::<Store>();
        let Ok(scale) = window.scale_factor() else {
            return;
        };
        let Ok(physical) = window.inner_size() else {
            return;
        };
        let Ok(scale_after) = window.scale_factor() else {
            return;
        };
        if scale != scale_after {
            return;
        }
        let Some(size) = logical_size(physical.width, physical.height, scale) else {
            return;
        };
        let (mode, current_generation) = {
            let session = store.mode.lock().unwrap();
            (session.name.clone(), session.generation)
        };
        let accept = was_editable
            && editable(&mode)
            && generation == current_generation
            && store.geometry_generation.load(Ordering::Acquire) == current_generation;
        diagnostics::record(
            "geometry-event",
            json!({
                "kind": kind, "mode": mode, "generation": generation,
                "physicalSize": [physical.width, physical.height], "scale": scale,
                "logicalSize": size, "acceptPreference": accept,
            }),
        );
        if !accept {
            return;
        }
        let position = window.outer_position().ok();
        let mut data = store.library.lock().unwrap();
        if kind == "resize" {
            data.settings.width = size.0;
            data.settings.height = size.1;
        }
        if let Some(p) = position {
            data.settings.x = Some(p.x as f64 / scale);
            data.settings.y = Some(p.y as f64 / scale);
        }
        let _ = persist(&store, &data);
        drop(data);
        if kind == "resize" {
            changed(window.app_handle());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct FakeWindow {
        sample: Cell<(u32, u32, f64)>,
        locked: Cell<bool>,
        restores: Cell<usize>,
    }
    impl GeometryWindow for FakeWindow {
        fn configure(&self, locked: bool, _: (f64, f64)) -> tauri::Result<()> {
            self.locked.set(locked);
            Ok(())
        }
        fn sample(&self) -> tauri::Result<(u32, u32, f64)> {
            Ok(self.sample.get())
        }
        fn resize(&self, size: (f64, f64)) -> tauri::Result<()> {
            let scale = self.sample.get().2;
            self.sample
                .set(((size.0 * scale) as u32, (size.1 * scale) as u32, scale));
            self.restores.set(self.restores.get() + 1);
            Ok(())
        }
    }
    #[test]
    fn hidden_display_rearrangement_is_restored_and_preview_can_still_resize() {
        let window = FakeWindow {
            sample: Cell::new((766, 1180, 2.)),
            locked: Cell::new(false),
            restores: Cell::new(0),
        };
        let mut guard = Protection::new();
        let expected = (383., 590.);
        assert!(guard.apply(&window, "hover", 1, expected).unwrap());
        assert!(window.locked.get());
        // Exact unexpected screen-sized rectangle captured in the user's log.
        window.sample.set((2940, 1626, 2.));
        guard.checked = Instant::now() - Duration::from_secs(1);
        assert!(!guard.apply(&window, "hover", 1, expected).unwrap());
        assert_eq!(window.sample.get(), (766, 1180, 2.));
        assert_eq!(window.restores.get(), 1);
        // A second display has a different scale but the same logical size.
        window.sample.set((383, 590, 1.));
        guard.checked = Instant::now() - Duration::from_secs(1);
        assert!(guard.apply(&window, "hover", 1, expected).unwrap());
        assert_eq!(window.restores.get(), 1);
        // Returning to preview repairs unexpected geometry before allowing edits.
        window.sample.set((1470, 813, 1.));
        assert!(!guard.apply(&window, "ready", 2, expected).unwrap());
        assert!(!window.locked.get());
        assert_eq!(window.sample.get(), (383, 590, 1.));
        assert!(guard.apply(&window, "ready", 2, expected).unwrap());
        window.sample.set((600, 300, 1.));
        guard.checked = Instant::now() - Duration::from_secs(1);
        guard.apply(&window, "ready", 2, expected).unwrap();
        assert_eq!(window.sample.get(), (600, 300, 1.));
        // Saving an intermediate drag size must not interrupt the next drag event.
        window.sample.set((650, 320, 1.));
        guard.checked = Instant::now() - Duration::from_secs(1);
        assert!(guard.apply(&window, "ready", 2, (600., 300.)).unwrap());
        assert_eq!(window.sample.get(), (650, 320, 1.));
        window.sample.set((600, 300, 1.));
        guard.apply(&window, "hover", 3, (600., 300.)).unwrap();
        assert!(window.locked.get());
        assert_eq!(window.sample.get(), (600, 300, 1.));
    }
    #[test]
    fn external_display_changes_preserve_logical_size_in_both_directions() {
        let expected = (383., 590.);
        for scale in [2., 1., 1.5, 2., 1., 2.] {
            let actual = logical_size(
                (expected.0 * scale) as u32,
                (expected.1 * scale) as u32,
                scale,
            )
            .unwrap();
            assert!(!differs(expected, actual));
        }
        assert!(differs(expected, logical_size(2940, 1626, 2.).unwrap()));
        assert!(differs(expected, logical_size(766, 1180, 1.).unwrap()));
    }
    #[test]
    fn hidden_and_reading_sizes_are_not_user_preferences() {
        for mode in ["hover", "paused"] {
            assert!(!editable(mode));
        }
        for mode in ["ready", "setup"] {
            assert!(editable(mode));
        }
        assert_eq!(logical_size(0, 0, 2.), None);
        assert_eq!(logical_size(766, 1180, 0.), None);
        assert_eq!(logical_size(766, 1180, f64::NAN), None);
    }
}
