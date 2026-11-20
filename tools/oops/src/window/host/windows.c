/* Standard Windows desktop host. The Rust caller owns artwork and timing. */
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <stdint.h>

typedef int32_t (*GoodhandUpdate)(void *context, const uint32_t **pixels);

enum { kTimer = 1, kPollMilliseconds = USER_TIMER_MINIMUM };
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

static void paint(HWND window, Player *player) {
    PAINTSTRUCT ps;
    RECT client;
    HDC dc = BeginPaint(window, &ps);
    if (!dc) {
        player->failed = TRUE;
        return;
    }
    if (GetClientRect(window, &client)) {
        HBRUSH black = (HBRUSH)GetStockObject(BLACK_BRUSH);
        int cw = client.right;
        int ch = client.bottom;
        if (!player->pixels || cw <= 0 || ch <= 0) {
            FillRect(dc, &client, black);
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
                if (top.bottom > top.top) FillRect(dc, &top, black);
                if (bottom.bottom > bottom.top) FillRect(dc, &bottom, black);
                if (left.right > left.left) FillRect(dc, &left, black);
                if (right.right > right.left) FillRect(dc, &right, black);
                SetStretchBltMode(dc, COLORONCOLOR);
                if (StretchDIBits(dc, x, y, w, h, 0, 0, player->width, player->height,
                                  player->pixels, &player->image, DIB_RGB_COLORS,
                                  SRCCOPY) == (int)GDI_ERROR) {
                    player->failed = TRUE;
                }
            } else {
                FillRect(dc, &client, black);
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
            poll(window, player);
            return 0;
        }
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
    case WM_KEYDOWN:
        if (wp == VK_ESCAPE || wp == 'Q') {
            DestroyWindow(window);
            return 0;
        }
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
                             uint32_t min_width, uint32_t min_height,
                             GoodhandUpdate update) {
    Player player = {0};
    WNDCLASSW cls = {0};
    RECT outer;
    DPI_AWARENESS_CONTEXT previous_dpi;
    HINSTANCE instance;
    HWND window = NULL;
    ATOM registered = 0;
    UINT dpi;
    int32_t result = 1;

    if (!update || !width || !height || !min_width || !min_height ||
        min_width > width || min_height > height || width > 32767u || height > 32767u) {
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
    if (!frame_size(player.width, player.height, dpi, &outer)) {
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
        if (!frame_size(player.width, player.height, dpi, &outer) ||
            !SetWindowPos(window, NULL, 0, 0, outer.right - outer.left, outer.bottom - outer.top,
                          SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)) {
            goto done;
        }
    }
    if (!SetTimer(window, kTimer, kPollMilliseconds, NULL)) {
        goto done;
    }
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
    if (registered) {
        UnregisterClassW(kClassName, instance);
    }
    SetThreadDpiAwarenessContext(previous_dpi);
    return result;
}
