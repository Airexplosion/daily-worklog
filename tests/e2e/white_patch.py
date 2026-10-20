"""在第 0 块屏（系统枚举顺序）正中放一个白色置顶小窗，挂几秒：给"截图没被压暗"那项检查一个亮的参照。用法：python -I white_patch.py 秒数"""
import ctypes, sys, time
from ctypes import wintypes

u32, g32, k32 = ctypes.windll.user32, ctypes.windll.gdi32, ctypes.windll.kernel32
u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
u32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u32.DefWindowProcW.restype = ctypes.c_ssize_t
WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)


class WNDCLASS(ctypes.Structure):
    _fields_ = [("style", wintypes.UINT), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                ("hInstance", wintypes.HINSTANCE), ("hIcon", wintypes.HICON), ("hCursor", wintypes.HANDLE), ("hbrBackground", wintypes.HBRUSH),
                ("lpszMenuName", wintypes.LPCWSTR), ("lpszClassName", wintypes.LPCWSTR)]


rects = []
PROC = ctypes.WINFUNCTYPE(ctypes.c_int, wintypes.HMONITOR, wintypes.HDC, ctypes.POINTER(wintypes.RECT), wintypes.LPARAM)
u32.EnumDisplayMonitors(None, None, PROC(lambda h, d, r, l: rects.append((r.contents.left, r.contents.top, r.contents.right, r.contents.bottom)) or 1), 0)
x0, y0, x1, y1 = rects[0]
cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
proc = WNDPROC(lambda h, m, w, l: u32.DefWindowProcW(h, m, w, l))
wc = WNDCLASS(0, proc, 0, 0, k32.GetModuleHandleW(None), None, None, g32.CreateSolidBrush(0xFFFFFF), None, "dwlWhitePatch")
u32.RegisterClassW(ctypes.byref(wc))
# WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE，WS_POPUP
hwnd = u32.CreateWindowExW(0x8 | 0x80 | 0x08000000, "dwlWhitePatch", "patch", 0x80000000, cx - 60, cy - 60, 120, 120, None, None, wc.hInstance, None)
u32.ShowWindow(hwnd, 4)
msg = wintypes.MSG()
end = time.time() + float(sys.argv[1])
while time.time() < end:
    while u32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
        u32.TranslateMessage(ctypes.byref(msg)); u32.DispatchMessageW(ctypes.byref(msg))
    time.sleep(0.01)
u32.DestroyWindow(hwnd)
