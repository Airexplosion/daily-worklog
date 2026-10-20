"""第 0 块屏（系统枚举顺序）中心点的颜色和这块屏的物理宽高：打印 [r, g, b, 宽, 高]"""
import ctypes, json
from ctypes import wintypes

u32, g32 = ctypes.windll.user32, ctypes.windll.gdi32
u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
rects = []
PROC = ctypes.WINFUNCTYPE(ctypes.c_int, wintypes.HMONITOR, wintypes.HDC, ctypes.POINTER(wintypes.RECT), wintypes.LPARAM)
u32.EnumDisplayMonitors(None, None, PROC(lambda h, d, r, l: rects.append((r.contents.left, r.contents.top, r.contents.right, r.contents.bottom)) or 1), 0)
x0, y0, x1, y1 = rects[0]
w, h = x1 - x0, y1 - y0
dc = u32.GetDC(None)
c = g32.GetPixel(dc, x0 + w // 2, y0 + h // 2)
u32.ReleaseDC(None, dc)
print(json.dumps([c & 255, (c >> 8) & 255, (c >> 16) & 255, w, h]))
