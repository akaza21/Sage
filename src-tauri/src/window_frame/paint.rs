use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, RECT},
        Graphics::Gdi::{
            BACKGROUND_MODE, BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW,
            CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
            DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_NORMAL, FillRect, HDC,
            OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
        UI::WindowsAndMessaging::GetClientRect,
    },
    core::w,
};

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CaptionPalette {
    pub background: u32,
    pub foreground: u32,
    pub hover: u32,
    pub pressed: u32,
    pub close_hover: u32,
    pub close_pressed: u32,
    pub close_foreground: u32,
    pub font_height: i32,
}

pub(super) unsafe fn button(
    hwnd: HWND,
    palette: CaptionPalette,
    glyph: u16,
    close: bool,
    hover: bool,
    pressed: bool,
) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        draw(hwnd, dc, palette, glyph, close, hover, pressed);
        let _ = EndPaint(hwnd, &paint);
    }
}

pub(super) unsafe fn draw(
    hwnd: HWND,
    dc: HDC,
    palette: CaptionPalette,
    glyph: u16,
    close: bool,
    hover: bool,
    pressed: bool,
) {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let background = match (close, hover, pressed) {
            (true, _, true) => palette.close_pressed,
            (true, true, _) => palette.close_hover,
            (false, _, true) => palette.pressed,
            (false, true, _) => palette.hover,
            _ => palette.background,
        };
        let foreground = if close && (hover || pressed) {
            palette.close_foreground
        } else {
            palette.foreground
        };
        let brush = CreateSolidBrush(COLORREF(background));
        let _ = FillRect(dc, &rect, brush);
        let _ = DeleteObject(brush.into());
        let font = CreateFontW(
            -palette.font_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Segoe MDL2 Assets"),
        );
        let previous_font = SelectObject(dc, font.into());
        let previous_background_mode = SetBkMode(dc, TRANSPARENT);
        let previous_text_color = SetTextColor(dc, COLORREF(foreground));
        let _ = DrawTextW(
            dc,
            &mut [glyph],
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let _ = SelectObject(dc, previous_font);
        let _ = SetBkMode(dc, BACKGROUND_MODE(previous_background_mode as u32));
        let _ = SetTextColor(dc, previous_text_color);
        let _ = DeleteObject(font.into());
    }
}
