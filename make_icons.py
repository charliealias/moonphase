import math
import struct
import zlib

def make_png(width, height, rgba_bytes):
    def chunk(tag, data):
        return (
            struct.pack("!I", len(data))
            + tag
            + data
            + struct.pack("!I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)  # Filter type 0 (None)
        raw.extend(rgba_bytes[y * stride : (y + 1) * stride])

    ihdr = struct.pack("!IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )

def blend(dst, src):
    sr, sg, sb, sa = src
    if sa <= 0.0:
        return dst
    if sa >= 1.0:
        return (sr, sg, sb, 1.0)
    dr, dg, db, da = dst
    oa = sa + da * (1.0 - sa)
    if oa <= 0.0:
        return (0.0, 0.0, 0.0, 0.0)
    return (
        (sr * sa + dr * da * (1.0 - sa)) / oa,
        (sg * sa + dg * da * (1.0 - sa)) / oa,
        (sb * sa + db * da * (1.0 - sa)) / oa,
        oa,
    )

def sd_circle(px, py, cx, cy, r):
    return math.hypot(px - cx, py - cy) - r

def sd_ellipse(px, py, cx, cy, rx, ry, angle=0.0):
    dx, dy = px - cx, py - cy
    ca, sa = math.cos(-angle), math.sin(-angle)
    qx = (dx * ca - dy * sa) / rx
    qy = (dx * sa + dy * ca) / ry
    return (math.hypot(qx, qy) - 1.0) * min(rx, ry)

def sd_segment(px, py, ax, ay, bx, by, r):
    pax, pay = px - ax, py - ay
    bax, bay = bx - ax, by - ay
    h = max(0.0, min(1.0, (pax * bax + pay * bay) / (bax * bax + bay * bay + 1e-9)))
    return math.hypot(pax - bax * h, pay - bay * h) - r

def cov(sd, aa=0.006):
    return max(0.0, min(1.0, 0.5 - sd / aa))

def sample_pixel(u, v):
    # u, v normalized to [-1, 1]
    # 1. 深邃夜空藍漸層背景 + 柔和星月光暈
    bg_t = max(0.0, min(1.0, (v + 1.0) * 0.5))
    col = (
        (16 + 14 * (1.0 - bg_t)) / 255.0,
        (23 + 22 * (1.0 - bg_t)) / 255.0,
        (42 + 34 * (1.0 - bg_t)) / 255.0,
        1.0,
    )
    # 中心暖光暈
    glow = max(0.0, 1.0 - math.hypot(u, v + 0.05) * 0.95)
    col = blend(col, (0.22, 0.35, 0.55, glow * 0.28))

    # ==========================================
    # 左側角色：Q 版小熊貓 (Red Panda) 中心 (-0.34, 0.14)
    # ==========================================
    rx, ry = -0.34, 0.14
    # 耳朵（白邊 + 暖棕內耳）
    for s in (-1, 1):
        ex, ey = rx + s * 0.25, ry - 0.25
        col = blend(col, (0.98, 0.96, 0.94, cov(sd_ellipse(u, v, ex, ey, 0.13, 0.15, s * 0.45))))
        col = blend(col, (0.45, 0.18, 0.12, cov(sd_ellipse(u, v, ex - s * 0.01, ey + 0.01, 0.08, 0.10, s * 0.45))))

    # 蓬鬆紅棕色圓臉
    col = blend(col, (0.85, 0.33, 0.18, cov(sd_ellipse(u, v, rx, ry, 0.34, 0.29))))
    # 臉頰兩側白色絨毛
    for s in (-1, 1):
        col = blend(col, (0.98, 0.96, 0.94, cov(sd_ellipse(u, v, rx + s * 0.21, ry + 0.08, 0.14, 0.11, s * -0.25))))
    # 眉毛白點（小熊貓招牌特徵）
    for s in (-1, 1):
        col = blend(col, (0.98, 0.96, 0.94, cov(sd_ellipse(u, v, rx + s * 0.11, ry - 0.10, 0.055, 0.032, s * -0.2))))
    # 白色口鼻區
    col = blend(col, (0.98, 0.96, 0.94, cov(sd_ellipse(u, v, rx, ry + 0.11, 0.13, 0.10))))
    # 深棕眼眶淚腺紋
    for s in (-1, 1):
        col = blend(col, (0.36, 0.13, 0.09, cov(sd_ellipse(u, v, rx + s * 0.13, ry + 0.04, 0.05, 0.09, s * 0.15))))
    # 圓滾滾黑眼睛 + 高光
    for s in (-1, 1):
        col = blend(col, (0.12, 0.08, 0.08, cov(sd_circle(u, v, rx + s * 0.13, ry - 0.01, 0.042))))
        col = blend(col, (1.00, 1.00, 1.00, cov(sd_circle(u, v, rx + s * 0.13 - 0.014, ry - 0.024, 0.015))))
    # 粉紅腮紅
    for s in (-1, 1):
        col = blend(col, (0.98, 0.45, 0.52, cov(sd_ellipse(u, v, rx + s * 0.22, ry + 0.06, 0.055, 0.03)) * 0.55))
    # 黑鼻子 & 微笑
    col = blend(col, (0.15, 0.10, 0.10, cov(sd_ellipse(u, v, rx, ry + 0.08, 0.036, 0.025))))
    col = blend(col, (0.25, 0.12, 0.10, cov(sd_segment(u, v, rx - 0.035, ry + 0.13, rx, ry + 0.145, 0.009))))
    col = blend(col, (0.25, 0.12, 0.10, cov(sd_segment(u, v, rx, ry + 0.145, rx + 0.035, ry + 0.13, 0.009))))

    # ==========================================
    # 右側角色：Q 版母獅子 (Lioness) 中心 (0.34, 0.14)
    # ==========================================
    lx, ly = 0.34, 0.14
    # 圓圓的獅子耳朵（無鬃毛、圓耳輪廓 + 深棕耳緣）
    for s in (-1, 1):
        ex, ey = lx + s * 0.24, ly - 0.22
        col = blend(col, (0.94, 0.72, 0.36, cov(sd_circle(u, v, ex, ey, 0.115))))
        col = blend(col, (0.48, 0.28, 0.14, cov(sd_circle(u, v, ex, ey, 0.068))))

    # 暖金沙色圓臉（母獅子俐落圓潤輪廓）
    col = blend(col, (0.95, 0.75, 0.40, cov(sd_ellipse(u, v, lx, ly, 0.32, 0.29))))
    # 額頭微亮毛色
    col = blend(col, (0.98, 0.84, 0.54, cov(sd_ellipse(u, v, lx, ly - 0.11, 0.18, 0.10)) * 0.5))
    # 奶油色口鼻區
    col = blend(col, (0.99, 0.94, 0.84, cov(sd_ellipse(u, v, lx, ly + 0.10, 0.14, 0.105))))
    # 靈動大眼（母獅子帶一點柔和眼睫毛感）
    for s in (-1, 1):
        col = blend(col, (0.16, 0.11, 0.08, cov(sd_circle(u, v, lx + s * 0.13, ly - 0.01, 0.043))))
        col = blend(col, (0.16, 0.11, 0.08, cov(sd_segment(u, v, lx + s * 0.16, ly - 0.025, lx + s * 0.19, ly - 0.04, 0.008))))
        col = blend(col, (1.00, 1.00, 1.00, cov(sd_circle(u, v, lx + s * 0.13 - 0.014, ly - 0.024, 0.015))))
    # 暖珊瑚腮紅
    for s in (-1, 1):
        col = blend(col, (0.96, 0.48, 0.45, cov(sd_ellipse(u, v, lx + s * 0.21, ly + 0.06, 0.055, 0.03)) * 0.5))
    # 粉棕愛心型小鼻子 & 貓科微笑嘴
    col = blend(col, (0.55, 0.24, 0.22, cov(sd_ellipse(u, v, lx, ly + 0.075, 0.042, 0.028))))
    col = blend(col, (0.35, 0.18, 0.12, cov(sd_segment(u, v, lx - 0.04, ly + 0.125, lx, ly + 0.14, 0.009))))
    col = blend(col, (0.35, 0.18, 0.12, cov(sd_segment(u, v, lx, ly + 0.14, lx + 0.04, ly + 0.125, 0.009))))

    # ==========================================
    # 頭頂中央：一彎小新月 + 暖粉小愛心 (呼應 MoonPhase 伴侶照護)
    # ==========================================
    # 小新月 (0.0, -0.46)
    moon_outer = sd_circle(u, v, -0.02, -0.46, 0.11)
    moon_inner = sd_circle(u, v, 0.04, -0.49, 0.095)
    crescent = max(moon_outer, -moon_inner)
    col = blend(col, (0.99, 0.88, 0.52, cov(crescent)))

    # 兩隻頭靠頭中間的小愛心 (0.0, -0.14)
    hx, hy = 0.0, -0.13
    heart_sd = min(
        sd_circle(u, v, hx - 0.026, hy - 0.015, 0.032),
        sd_circle(u, v, hx + 0.026, hy - 0.015, 0.032),
    )
    heart_tip = sd_ellipse(u, v, hx, hy + 0.015, 0.042, 0.045, 0.785)
    col = blend(col, (0.98, 0.44, 0.55, cov(min(heart_sd, heart_tip))))

    return (
        int(max(0, min(255, round(col[0] * 255)))),
        int(max(0, min(255, round(col[1] * 255)))),
        int(max(0, min(255, round(col[2] * 255)))),
        255,
    )

def render_icon(size, out_path):
    buf = bytearray(size * size * 4)
    # 2x2 超採樣抗鋸齒 (SSAA)
    offsets = ((-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25))
    idx = 0
    for y in range(size):
        for x in range(size):
            r_acc = g_acc = b_acc = 0
            for ox, oy in offsets:
                u = ((x + 0.5 + ox) / size) * 2.0 - 1.0
                v = ((y + 0.5 + oy) / size) * 2.0 - 1.0
                r, g, b, _ = sample_pixel(u, v)
                r_acc += r
                g_acc += g
                b_acc += b
            buf[idx] = r_acc // 4
            buf[idx + 1] = g_acc // 4
            buf[idx + 2] = b_acc // 4
            buf[idx + 3] = 255
            idx += 4

    with open(out_path, "wb") as f:
        f.write(make_png(size, size, buf))
    print(f"Generated {out_path} ({size}x{size})")

if __name__ == "__main__":
    render_icon(192, "dist/icon-192.png")
    render_icon(512, "dist/icon-512.png")