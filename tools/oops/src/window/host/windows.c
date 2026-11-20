/* Standard Windows desktop host. The Rust caller owns artwork and timing. */
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <dwmapi.h>
#include <stdint.h>

typedef int32_t (*GoodhandUpdate)(void *context, const uint32_t **pixels);

enum { kTimer = 1, kPollMilliseconds = USER_TIMER_MINIMUM, kKeyRevealMs = 1500 };
// Documented DWM per-window colors on Windows 11. Use the numeric IDs
// so older headers can still build the Windows 10 fallback.
enum { kCornerPreference = 33, kBorderColor = 34, kCaptionColor = 35, kCaptionText = 36 };
static const wchar_t kMaskClass[] = L"GoodhandCaptionMask";
static const wchar_t kClassName[] = L"GoodhandNativeWindow";

typedef struct {
    void *context;
    GoodhandUpdate update;
    const uint32_t *pixels;
    BITMAPINFO image;
    int32_t width;
    int32_t height;
    int32_t min_width;
    int32_t min_height;
    COLORREF backdrop;
    HBRUSH backdrop_brush;
    BOOL use_native_colors;
    BOOL revealed;
    BOOL menu_active;
    BOOL sizing;
    ULONGLONG keyboard_until;
    HWND caption_mask;
    RECT mask_rect;
    BOOL mask_visible;
    BOOL failed;
    BOOL closed;
} Player;

static BOOL frame_size(int width, int height, UINT dpi, RECT *outer) {
    outer->left = outer->top = 0;
    outer->right = MulDiv(width, (int)dpi, USER_DEFAULT_SCREEN_DPI);
    outer->bottom = MulDiv(height, (int)dpi, USER_DEFAULT_SCREEN_DPI);
    return outer->right > 0 && outer->bottom > 0 &&
        AdjustWindowRectExForDpi(outer, WS_OVERLAPPEDWINDOW, FALSE, 0, dpi);
}

static BOOL high_contrast(void) {
    HIGHCONTRASTW setting = {0};
    setting.cbSize = (UINT)sizeof(setting);
    return !SystemParametersInfoW(SPI_GETHIGHCONTRAST, setting.cbSize, &setting, 0)
        || (setting.dwFlags & HCF_HIGHCONTRASTON) != 0;
}

static BOOL frame_colors(HWND window, COLORREF fill, COLORREF line, COLORREF text) {
    return SUCCEEDED(DwmSetWindowAttribute(window, kCaptionColor, &fill, (DWORD)sizeof(fill)))
        && SUCCEEDED(DwmSetWindowAttribute(window, kBorderColor, &line, (DWORD)sizeof(line)))
        && SUCCEEDED(DwmSetWindowAttribute(window, kCaptionText, &text, (DWORD)sizeof(text)));
}

static void restore_frame_colors(HWND window) {
    const COLORREF system_default = 0xffffffffu;
    DwmSetWindowAttribute(window, kCaptionColor, &system_default, (DWORD)sizeof(system_default));
    DwmSetWindowAttribute(window, kBorderColor, &system_default, (DWORD)sizeof(system_default));
    DwmSetWindowAttribute(window, kCaptionText, &system_default, (DWORD)sizeof(system_default));
}

static LRESULT CALLBACK mask_proc(HWND window, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_NCCREATE) {
        CREATESTRUCTW *params = (CREATESTRUCTW *)lp;
        SetLastError(ERROR_SUCCESS);
        if (!SetWindowLongPtrW(window, GWLP_USERDATA, (LONG_PTR)params->lpCreateParams) &&
            GetLastError() != ERROR_SUCCESS) {
            return FALSE;
        }
    }
    if (msg == WM_NCHITTEST) return HTTRANSPARENT;
    if (msg == WM_MOUSEACTIVATE) return MA_NOACTIVATE;
    if (msg == WM_GETOBJECT) return 0;  /* Do not supply a separate accessibility provider. */
    if (msg == WM_ERASEBKGND) return 1;
    if (msg == WM_PAINT) {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(window, &ps);
        HBRUSH brush = (HBRUSH)GetWindowLongPtrW(window, GWLP_USERDATA);
        if (dc && brush) FillRect(dc, &ps.rcPaint, brush);
        EndPaint(window, &ps);
        return 0;
    }
    return DefWindowProcW(window, msg, wp, lp);
}

static void hide_mask(Player *player) {
    if (player->caption_mask && player->mask_visible) {
        ShowWindow(player->caption_mask, SW_HIDE);
        player->mask_visible = FALSE;
    }
}

static void restore_frame(HWND owner, Player *player) {
    const DWORD system_corners = 0; /* DWMWCP_DEFAULT */
    hide_mask(player);
    restore_frame_colors(owner);
    DwmSetWindowAttribute(owner, kCornerPreference, &system_corners, (DWORD)sizeof(system_corners));
    player->use_native_colors = FALSE;
}

static void prepare_frame(HWND window, Player *player) {
    const DWORD square_corners = 1; /* DWMWCP_DONOTROUND */
    restore_frame(window, player);
    if (high_contrast() || !player->caption_mask) return;
    // Both surfaces must be square; a plain popup cannot inherit the
    // owner's rounded higher-order clip. Styles and client geometry stay.
    player->use_native_colors =
        SUCCEEDED(DwmSetWindowAttribute(window, kCornerPreference,
                                        &square_corners, (DWORD)sizeof(square_corners))) &&
        frame_colors(window, player->backdrop, player->backdrop, player->backdrop);
    if (!player->use_native_colors) restore_frame(window, player);
}

static BOOL caption_box(HWND owner, RECT *out) {
    RECT frame;
    POINT client = {0};
    DWORD cloaked = 0;
    if (!IsWindowVisible(owner) || IsIconic(owner) ||
        FAILED(DwmGetWindowAttribute(owner, DWMWA_CLOAKED, &cloaked, (DWORD)sizeof(cloaked))) ||
        cloaked ||
        FAILED(DwmGetWindowAttribute(owner, DWMWA_EXTENDED_FRAME_BOUNDS,
                                     &frame, (DWORD)sizeof(frame))) ||
        !ClientToScreen(owner, &client)) return FALSE;
    // Both coordinates are physical pixels in this Per-Monitor V2 thread.
    // Only the visible caption strip, never the pane or invisible hit zones.
    frame.bottom = client.y < frame.bottom ? client.y : frame.bottom;
    if (frame.left >= frame.right || frame.top >= frame.bottom) return FALSE;
    if (IsZoomed(owner)) {
        // The compositor may clip maximized borders to the monitor work
        // area. Never let the auxiliary strip cover those clipped pixels.
        MONITORINFO area = {0};
        RECT clipped;
        HMONITOR monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
        area.cbSize = (DWORD)sizeof(area);
        if (!monitor || !GetMonitorInfoW(monitor, &area) ||
            !IntersectRect(&clipped, &frame, &area.rcWork)) return FALSE;
        frame = clipped;
    }
    *out = frame;
    return TRUE;
}

static void follow_pointer(HWND window, Player *player) {
    if (!player->use_native_colors) return;
    RECT caption;
    if (!caption_box(window, &caption)) {
        hide_mask(player);
        return;
    }
    POINT p;
    if (!GetCursorPos(&p)) {
        hide_mask(player);
        return;
    }
    HWND under = WindowFromPoint(p);
    BOOL reveal = player->menu_active || player->sizing
        || GetTickCount64() < player->keyboard_until
        || (under && GetAncestor(under, GA_ROOT) == window)
        || under == player->caption_mask;
    if (reveal) {
        hide_mask(player);
        if (!player->revealed && !frame_colors(window, RGB(8,10,11),
                                                RGB(53,59,61), RGB(225,229,231))) {
            restore_frame(window, player);
            return;
        }
    } else {
        if (player->revealed && !frame_colors(window, player->backdrop,
                                              player->backdrop, player->backdrop)) {
            restore_frame(window, player);
            return;
        }
        if (!player->mask_visible || !EqualRect(&caption, &player->mask_rect)) {
            if (!SetWindowPos(player->caption_mask, NULL, caption.left, caption.top,
                              caption.right - caption.left, caption.bottom - caption.top,
                              SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW)) {
                restore_frame(window, player);
                return;
            }
            player->mask_rect = caption;
            player->mask_visible = TRUE;
        }
    }
    player->revealed = reveal;
}

static void paint(HWND window, Player *player) {
    PAINTSTRUCT ps;
    RECT client;
    HDC dc = BeginPaint(window, &ps);
    if (!dc) {
        player->failed = TRUE;
        return;
    }
    if (GetClientRect(window, &client)) {
        HBRUSH backdrop = player->backdrop_brush;
        int cw = client.right;
        int ch = client.bottom;
        if (!player->pixels || cw <= 0 || ch <= 0) {
            FillRect(dc, &client, backdrop);
        } else {
            int w = cw;
            int h = ch;
            /* Integer contain fit, with no aspect change except rounding. */
            if ((int64_t)cw * player->height <= (int64_t)ch * player->width) {
                h = (int)((int64_t)cw * player->height / player->width);
            } else {
                w = (int)((int64_t)ch * player->width / player->height);
            }
            if (w > 0 && h > 0) {
                int x = (cw - w) / 2;
                int y = (ch - h) / 2;
                /* Paint only the bars before the opaque image; avoid a black
                   clear across the artwork on every frame. */
                RECT top = {0, 0, cw, y};
                RECT bottom = {0, y + h, cw, ch};
                RECT left = {0, y, x, y + h};
                RECT right = {x + w, y, cw, y + h};
                if (top.bottom > top.top) FillRect(dc, &top, backdrop);
                if (bottom.bottom > bottom.top) FillRect(dc, &bottom, backdrop);
                if (left.right > left.left) FillRect(dc, &left, backdrop);
                if (right.right > right.left) FillRect(dc, &right, backdrop);
                SetStretchBltMode(dc, COLORONCOLOR);
                if (StretchDIBits(dc, x, y, w, h, 0, 0, player->width, player->height,
                                  player->pixels, &player->image, DIB_RGB_COLORS,
                                  SRCCOPY) == (int)GDI_ERROR) {
                    player->failed = TRUE;
                }
            } else {
                FillRect(dc, &client, backdrop);
            }
        }
    }
    EndPaint(window, &ps);
}

static void poll(HWND window, Player *player) {
    const uint32_t *pixels = player->pixels;
    int32_t status = player->update(player->context, &pixels);
    if (status < 0) {
        DestroyWindow(window);
    } else if (status == 1 && pixels) {
        player->pixels = pixels;
        if (!IsIconic(window)) {
            InvalidateRect(window, NULL, FALSE);
        }
    } else if (status != 0) {
        player->failed = TRUE;
        DestroyWindow(window);
    }
}

static LRESULT CALLBACK window_proc(HWND window, UINT msg, WPARAM wp, LPARAM lp) {
    Player *player = (Player *)GetWindowLongPtrW(window, GWLP_USERDATA);
    if (msg == WM_NCCREATE) {
        CREATESTRUCTW *params = (CREATESTRUCTW *)lp;
        SetLastError(ERROR_SUCCESS);
        if (!SetWindowLongPtrW(window, GWLP_USERDATA, (LONG_PTR)params->lpCreateParams) &&
            GetLastError() != ERROR_SUCCESS) {
            return FALSE;
        }
        return DefWindowProcW(window, msg, wp, lp);
    }
    if (!player) {
        return DefWindowProcW(window, msg, wp, lp);
    }
    switch (msg) {
    case WM_GETMINMAXINFO: {
        RECT outer;
        MINMAXINFO *limits = (MINMAXINFO *)lp;
        if (frame_size(player->min_width, player->min_height, GetDpiForWindow(window), &outer)) {
            LONG width = outer.right - outer.left;
            LONG height = outer.bottom - outer.top;
            if (limits->ptMinTrackSize.x < width) limits->ptMinTrackSize.x = width;
            if (limits->ptMinTrackSize.y < height) limits->ptMinTrackSize.y = height;
        }
        return 0;
    }
    case WM_DPICHANGED: {
        const RECT *suggested = (const RECT *)lp;
        SetWindowPos(window, NULL, suggested->left, suggested->top,
                     suggested->right - suggested->left, suggested->bottom - suggested->top,
                     SWP_NOZORDER | SWP_NOACTIVATE);
        return 0;
    }
    case WM_TIMER:
        if (wp == kTimer) {
            follow_pointer(window, player);
            poll(window, player);
            return 0;
        }
        break;
    case WM_WINDOWPOSCHANGING:
        // A stale popup must never show in the previous screen location.
        hide_mask(player);
        break;
    case WM_ACTIVATE:
        player->keyboard_until = LOWORD(wp) == WA_ACTIVE
            ? GetTickCount64() + kKeyRevealMs : 0;
        follow_pointer(window, player);
        break;
    case WM_NCMOUSEMOVE:
    case WM_MOUSEMOVE:
        player->keyboard_until = 0;
        follow_pointer(window, player);
        break;
    case WM_ENTERSIZEMOVE:
    case WM_EXITSIZEMOVE:
        player->sizing = msg == WM_ENTERSIZEMOVE;
        follow_pointer(window, player);
        break;
    case WM_SYSCOLORCHANGE:
    case WM_THEMECHANGED:
    case WM_SETTINGCHANGE:
        prepare_frame(window, player);
        follow_pointer(window, player);
        break;
    case WM_ENTERMENULOOP:
    case WM_EXITMENULOOP:
        // Keep the caption legible when using the real keyboard system menu.
        player->menu_active = msg == WM_ENTERMENULOOP;
        follow_pointer(window, player);
        break;
    case WM_SIZE:
        if (wp != SIZE_MINIMIZED) {
            InvalidateRect(window, NULL, FALSE);
        }
        return 0;
    case WM_PAINT:
        paint(window, player);
        if (player->failed) {
            DestroyWindow(window);
        }
        return 0;
    case WM_ERASEBKGND:
        return 1; /* WM_PAINT covers the entire client. */
    case WM_SYSKEYDOWN:
        player->keyboard_until = GetTickCount64() + kKeyRevealMs;
        follow_pointer(window, player);
        break;
    case WM_KEYDOWN:
        if (wp == VK_ESCAPE || wp == 'Q') {
            DestroyWindow(window);
            return 0;
        }
        player->keyboard_until = GetTickCount64() + kKeyRevealMs;
        follow_pointer(window, player);
        break;
    case WM_DESTROY:
        KillTimer(window, kTimer);
        player->closed = TRUE;
        /* No WM_QUIT is needed: this host owns the message loop and observes
           closed when DispatchMessage returns. Avoid leaving a queued quit. */
        return 0;
    case WM_NCDESTROY:
        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        break;
    }
    return DefWindowProcW(window, msg, wp, lp);
}

int32_t goodhand_run_platform(void *context, uint32_t width, uint32_t height,
                             uint32_t client_width, uint32_t client_height,
                             uint32_t min_width, uint32_t min_height,
                             uint32_t background, GoodhandUpdate update) {
    Player player = {0};
    WNDCLASSW cls = {0};
    WNDCLASSW mask_cls = {0};
    RECT outer;
    DPI_AWARENESS_CONTEXT previous_dpi;
    HINSTANCE instance = NULL;
    HWND window = NULL;
    ATOM registered = 0;
    ATOM mask_registered = 0;
    UINT dpi;
    int32_t result = 1;

    if (!update || !width || !height || !min_width || !min_height ||
        min_width > client_width || min_height > client_height ||
        client_width > 32767u || client_height > 32767u ||
        width > 32767u || height > 32767u) {
        return result;
    }

    /* Per-monitor V2 retains a logical client size and native system chrome.
       It requires Windows 10 version 1703 or newer. */
    previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    if (!previous_dpi) {
        return result;
    }

    player.context = context;
    player.update = update;
    player.width = (int32_t)width;
    player.height = (int32_t)height;
    player.min_width = (int32_t)min_width;
    player.min_height = (int32_t)min_height;
    player.backdrop = RGB((background >> 16) & 0xff, (background >> 8) & 0xff, background & 0xff);
    player.backdrop_brush = CreateSolidBrush(player.backdrop);
    if (!player.backdrop_brush) goto done;
    player.image.bmiHeader.biSize = (DWORD)sizeof(BITMAPINFOHEADER);
    player.image.bmiHeader.biWidth = player.width;
    player.image.bmiHeader.biHeight = -player.height; /* Top-down BGRX. */
    player.image.bmiHeader.biPlanes = 1;
    player.image.bmiHeader.biBitCount = 32;
    player.image.bmiHeader.biCompression = BI_RGB;

    instance = GetModuleHandleW(NULL);
    cls.lpfnWndProc = window_proc;
    cls.hInstance = instance;
    cls.hCursor = LoadCursorW(NULL, MAKEINTRESOURCEW(32512)); /* IDC_ARROW */
    cls.lpszClassName = kClassName;
    registered = RegisterClassW(&cls);
    if (!registered) {
        goto done;
    }
    dpi = GetDpiForSystem();
    if (!frame_size((int)client_width, (int)client_height, dpi, &outer)) {
        goto done;
    }
    window = CreateWindowExW(0, kClassName, L"oops", WS_OVERLAPPEDWINDOW,
                             CW_USEDEFAULT, CW_USEDEFAULT,
                             outer.right - outer.left, outer.bottom - outer.top,
                             NULL, NULL, instance, &player);
    if (!window) {
        goto done;
    }
    /* The default location may be on a different monitor from the system DPI. */
    if (GetDpiForWindow(window) != dpi) {
        dpi = GetDpiForWindow(window);
        if (!frame_size((int)client_width, (int)client_height, dpi, &outer) ||
            !SetWindowPos(window, NULL, 0, 0, outer.right - outer.left, outer.bottom - outer.top,
                          SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)) {
            goto done;
        }
    }
    if (!SetTimer(window, kTimer, kPollMilliseconds, NULL)) {
        goto done;
    }
    mask_cls.lpfnWndProc = mask_proc;
    mask_cls.hInstance = instance;
    mask_cls.lpszClassName = kMaskClass;
    mask_registered = RegisterClassW(&mask_cls);
    if (mask_registered) {
        player.caption_mask = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            kMaskClass, L"", WS_POPUP, 0, 0, 1, 1, window, NULL, instance,
            player.backdrop_brush);
        if (player.caption_mask) {
            const DWORD square_corners = 1;
            if (!SetLayeredWindowAttributes(player.caption_mask, 0, 255, LWA_ALPHA) ||
                FAILED(DwmSetWindowAttribute(player.caption_mask, kCornerPreference,
                                            &square_corners, (DWORD)sizeof(square_corners)))) {
                DestroyWindow(player.caption_mask);
                player.caption_mask = NULL;
            }
        }
    }
    prepare_frame(window, &player);
    ShowWindow(window, SW_SHOWNORMAL);
    if (!player.closed) {
        poll(window, &player);
    }
    if (!player.closed) {
        UpdateWindow(window);
    }
    while (!player.closed) {
        MSG msg;
        BOOL status = GetMessageW(&msg, NULL, 0, 0);
        if (status < 0) {
            player.failed = TRUE;
            break;
        }
        if (!status) {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    result = player.failed ? 1 : 0;

done:
    if (window && !player.closed) {
        DestroyWindow(window);
    }
    if (mask_registered) UnregisterClassW(kMaskClass, instance);
    if (registered) {
        UnregisterClassW(kClassName, instance);
    }
    if (player.backdrop_brush) DeleteObject(player.backdrop_brush);
    SetThreadDpiAwarenessContext(previous_dpi);
    return result;
}
