//! Avoid Tao's generic style/size paths: they can call ShowWindow and raise a
//! covered reader. Every operation here preserves focus and window stacking.
use std::{io, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND, RECT},
    UI::WindowsAndMessaging::{
        GetClientRect, GetWindowLongW, GetWindowRect, IsWindow, SetWindowLongW, SetWindowPos,
        GWL_STYLE, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_THICKFRAME,
    },
};

pub(super) fn configure(window: HWND, locked: bool) -> io::Result<()> {
    unsafe {
        if IsWindow(window) == 0 {
            return Err(io::Error::other("Reader window no longer exists"));
        }
        let current = GetWindowLongW(window, GWL_STYLE) as u32;
        let mut desired = current & !(WS_MAXIMIZE | WS_MAXIMIZEBOX);
        if locked {
            desired &= !WS_THICKFRAME;
        } else {
            desired |= WS_THICKFRAME;
        }
        if current != desired {
            SetLastError(0);
            if SetWindowLongW(window, GWL_STYLE, desired as i32) == 0 && GetLastError() != 0 {
                return Err(io::Error::last_os_error());
            }
            if SetWindowPos(
                window,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_ASYNCWINDOWPOS
                    | SWP_FRAMECHANGED
                    | SWP_NOACTIVATE
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

pub(super) fn resize(window: HWND, width: i32, height: i32) -> io::Result<()> {
    unsafe {
        let mut client = RECT::default();
        let mut outer = RECT::default();
        if GetClientRect(window, &mut client) == 0 || GetWindowRect(window, &mut outer) == 0 {
            return Err(io::Error::last_os_error());
        }
        let border_width = (outer.right - outer.left) - (client.right - client.left);
        let border_height = (outer.bottom - outer.top) - (client.bottom - client.top);
        if SetWindowPos(
            window,
            null_mut(),
            0,
            0,
            width + border_width,
            height + border_height,
            SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindow, GW_HWNDPREV, HWND_TOP, SWP_SHOWWINDOW, WS_POPUP,
    };
    struct TestWindow(HWND);
    impl TestWindow {
        fn new() -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let window = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_POPUP | WS_THICKFRAME | WS_MAXIMIZEBOX,
                    10000,
                    10000,
                    480,
                    240,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            assert!(!window.is_null());
            Self(window)
        }
    }
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    fn above(top: HWND, bottom: HWND) -> bool {
        let mut next = bottom;
        for _ in 0..512 {
            next = unsafe { GetWindow(next, GW_HWNDPREV) };
            if next.is_null() {
                return false;
            }
            if next == top {
                return true;
            }
        }
        false
    }
    #[test]
    fn locking_and_repairing_dimensions_preserve_a_covering_window() {
        let reader = TestWindow::new();
        let cover = TestWindow::new();
        unsafe {
            SetWindowPos(
                reader.0,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            SetWindowPos(
                cover.0,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        }
        configure(reader.0, true).unwrap();
        resize(reader.0, 383, 590).unwrap();
        unsafe {
            assert_eq!(
                GetWindowLongW(reader.0, GWL_STYLE) as u32 & (WS_THICKFRAME | WS_MAXIMIZEBOX),
                0
            );
            let mut client = RECT::default();
            assert_ne!(GetClientRect(reader.0, &mut client), 0);
            assert_eq!(
                (client.right - client.left, client.bottom - client.top),
                (383, 590)
            );
            assert!(above(cover.0, reader.0));
        }
        configure(reader.0, false).unwrap();
        unsafe {
            assert_ne!(
                GetWindowLongW(reader.0, GWL_STYLE) as u32 & WS_THICKFRAME,
                0
            );
            assert!(above(cover.0, reader.0));
        }
    }
}
