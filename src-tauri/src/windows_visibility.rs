use std::collections::HashSet;
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
    UI::WindowsAndMessaging::{
        GetWindow, GetWindowLongW, GetWindowRect, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE,
        GW_HWNDPREV, WS_EX_TRANSPARENT,
    },
};

// A transparent reader is skipped by normal hit testing. Inspect only windows
// ABOVE it, so the application underneath cannot prevent an exposed reader waking.
pub(super) fn can_reveal(reader: HWND, point: (i32, i32)) -> bool {
    unsafe {
        if IsWindow(reader) == 0 {
            return false;
        }
        let mut above = GetWindow(reader, GW_HWNDPREV);
        let mut seen = HashSet::new();
        while !above.is_null() {
            // Windows can disappear/reorder while enumerating. Fail closed on cycles.
            if !seen.insert(above as usize) || seen.len() > 512 {
                return false;
            }
            if IsWindowVisible(above) != 0
                && IsIconic(above) == 0
                && GetWindowLongW(above, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT == 0
            {
                let mut bounds = RECT::default();
                if GetWindowRect(above, &mut bounds) != 0
                    && point.0 >= bounds.left
                    && point.0 < bounds.right
                    && point.1 >= bounds.top
                    && point.1 < bounds.bottom
                {
                    let mut cloaked: u32 = 0;
                    DwmGetWindowAttribute(
                        above,
                        DWMWA_CLOAKED as u32,
                        &mut cloaked as *mut u32 as _,
                        std::mem::size_of::<u32>() as u32,
                    );
                    if cloaked == 0 {
                        return false;
                    }
                }
            }
            above = GetWindow(above, GW_HWNDPREV);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SetWindowPos, ShowWindow, HWND_BOTTOM, HWND_TOP,
        SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WS_EX_TOPMOST, WS_POPUP,
    };
    struct TestWindow(HWND);
    impl TestWindow {
        fn new() -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let name: Vec<u16> = "fishbaby-visibility-test\0".encode_utf16().collect();
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    name.as_ptr(),
                    WS_POPUP,
                    100,
                    100,
                    200,
                    200,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            assert!(!hwnd.is_null());
            Self(hwnd)
        }
        fn place(&self, after: HWND, x: i32, y: i32) {
            assert_ne!(
                unsafe {
                    SetWindowPos(
                        self.0,
                        after,
                        x,
                        y,
                        200,
                        200,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    )
                },
                0
            );
        }
    }
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    #[test]
    fn normal_windows_above_reader_block_hover_but_windows_below_do_not() {
        let reader = TestWindow::new();
        let other = TestWindow::new();
        reader.place(HWND_TOP, 100, 100);
        other.place(HWND_BOTTOM, 100, 100);
        assert!(can_reveal(reader.0, (150, 150)));
        assert_eq!(
            unsafe { GetWindowLongW(reader.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST,
            0
        );
        other.place(HWND_TOP, 100, 100);
        assert!(!can_reveal(reader.0, (150, 150)));
        other.place(HWND_TOP, 400, 400);
        assert!(can_reveal(reader.0, (150, 150)));
        other.place(HWND_TOP, 100, 100);
        unsafe {
            ShowWindow(other.0, SW_HIDE);
        }
        assert!(can_reveal(reader.0, (150, 150)));
    }
}
