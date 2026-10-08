use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::sync::mpsc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SM_CXSCREEN,
    SM_CYSCREEN, SW_SHOW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WNDCLASSW,
    WS_CAPTION, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::w;
use wry::WebViewBuilder;

use crate::auth::oauth::authorization_code_from_redirect;
use crate::error::Error;

thread_local! {
    static SIGN_IN_RESULT: RefCell<Option<Result<String, String>>> = const { RefCell::new(None) };
}

pub fn capture_code(url: &str) -> Result<String, Error> {
    let (tx, rx) = mpsc::channel();
    let url = url.to_owned();
    std::thread::Builder::new()
        .name("kilog-signin".into())
        .spawn(move || {
            let _ = tx.send(sign_in_window(&url));
        })
        .map_err(|err| Error::OAuth(format!("could not open the sign-in window: {err}")))?;
    rx.recv()
        .map_err(|_| Error::OAuth("sign-in window stopped".into()))?
}

fn sign_in_window(url: &str) -> Result<String, Error> {
    unsafe {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    SIGN_IN_RESULT.with(|slot| *slot.borrow_mut() = None);

    let hwnd = create_window()?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    let host = HostWindow(hwnd);
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_navigation_handler(|uri: String| {
            if let Some(result) = authorization_code_from_redirect(&uri) {
                SIGN_IN_RESULT.with(|slot| {
                    *slot.borrow_mut() = Some(result.map_err(|err| err.to_string()));
                });
                unsafe { PostQuitMessage(0) };
                false
            } else {
                true
            }
        })
        .build(&host)
        .map_err(|err| Error::OAuth(format!("could not open the sign-in window: {err}")))?;

    pump_messages();
    drop(webview);
    unsafe {
        let _ = DestroyWindow(hwnd);
    }

    match SIGN_IN_RESULT.with(|slot| slot.borrow_mut().take()) {
        Some(Ok(code)) => Ok(code),
        Some(Err(err)) => Err(Error::OAuth(err)),
        None => Err(Error::OAuth("sign-in window was closed".into())),
    }
}

fn create_window() -> Result<HWND, Error> {
    unsafe {
        let instance = GetModuleHandleW(None)
            .map_err(|err| Error::OAuth(format!("could not open the sign-in window: {err}")))?;
        let class_name = w!("KilogSignIn");
        let cursor = LoadCursorW(None, IDC_ARROW)
            .map_err(|err| Error::OAuth(format!("could not open the sign-in window: {err}")))?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            hCursor: cursor,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {}
        let width = 566;
        let height = 700;
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_width - width).max(0) / 2;
        let y = (screen_height - height).max(0) / 2;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Sign in"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            if screen_width == 0 { CW_USEDEFAULT } else { x },
            if screen_height == 0 { CW_USEDEFAULT } else { y },
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|err| Error::OAuth(format!("could not open the sign-in window: {err}")))?;
        Ok(hwnd)
    }
}

fn pump_messages() {
    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        SIGN_IN_RESULT.with(|slot| {
            if slot.borrow().is_none() {
                *slot.borrow_mut() = Some(Err("sign-in window was closed".into()));
            }
        });
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct HostWindow(HWND);

impl HasWindowHandle for HostWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(hwnd);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for HostWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
        })
    }
}
