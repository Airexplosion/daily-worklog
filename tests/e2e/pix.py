"""读 PNG 的尺寸和几个点的颜色：python -I pix.py 文件 x,y x,y ..."""
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGBA")
pts = [tuple(int(v) for v in a.split(",")) for a in sys.argv[2:]]
print(json.dumps({"w": im.width, "h": im.height, "px": [list(im.getpixel(p)) for p in pts]}))
