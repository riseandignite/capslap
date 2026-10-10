#!/usr/bin/env python3
"""
Generate PyCapSlap app icons in multiple resolutions based on refu.png reference.
Produces:
  - Master high-res 1024x1024 PNG (with transparent background & native macOS shadow)
  - Standard sizes: 16, 24, 32, 48, 64, 128, 256, 512, 1024
  - macOS .icns file (via iconutil)
  - Windows .ico file
  - Vector .svg icon
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from PySide6.QtCore import QBuffer, QIODevice, QPointF, QRectF, Qt
from PySide6.QtGui import (
    QBrush,
    QColor,
    QImage,
    QLinearGradient,
    QPainter,
    QPainterPath,
    QPen,
)
from PySide6.QtWidgets import QApplication


def create_master_icon(size: int = 1024) -> QImage:
    # Use ARGB32 with Premultiplied Alpha for smooth antialiasing
    img = QImage(size, size, QImage.Format.Format_ARGB32_Premultiplied)
    img.fill(Qt.GlobalColor.transparent)

    painter = QPainter(img)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)

    scale = size / 1024.0

    # Squircle geometry (macOS Big Sur / Sonoma standard icon proportions)
    # On 1024x1024: 824x824 squircle, 100px margins, radius 185
    sq_x = 100.0 * scale
    sq_y = 80.0 * scale
    sq_w = 824.0 * scale
    sq_h = 824.0 * scale
    sq_r = 185.0 * scale
    sq_rect = QRectF(sq_x, sq_y, sq_w, sq_h)

    # 1. Soft ambient drop shadow underneath squircle
    shadow_layers = [
        (40.0 * scale, 12.0 * scale, QColor(0, 0, 0, 16)),
        (30.0 * scale, 10.0 * scale, QColor(0, 0, 0, 24)),
        (20.0 * scale, 8.0 * scale, QColor(0, 0, 0, 36)),
        (12.0 * scale, 6.0 * scale, QColor(0, 0, 0, 48)),
        (6.0 * scale, 4.0 * scale, QColor(0, 0, 0, 60)),
    ]
    for blur_expand, offset_y, color in shadow_layers:
        s_rect = QRectF(
            sq_x - blur_expand * 0.3,
            sq_y + offset_y,
            sq_w + blur_expand * 0.6,
            sq_h + blur_expand * 0.3,
        )
        s_path = QPainterPath()
        s_path.addRoundedRect(
            s_rect, sq_r + blur_expand * 0.2, sq_r + blur_expand * 0.2
        )
        painter.fillPath(s_path, color)

    # 2. Main Outer Squircle Frame
    squircle_path = QPainterPath()
    squircle_path.addRoundedRect(sq_rect, sq_r, sq_r)

    # Outer frame gradient (dark slate frame)
    frame_grad = QLinearGradient(0, sq_y, 0, sq_y + sq_h)
    frame_grad.setColorAt(0.0, QColor("#222734"))
    frame_grad.setColorAt(0.5, QColor("#191d26"))
    frame_grad.setColorAt(1.0, QColor("#12141c"))
    painter.fillPath(squircle_path, frame_grad)

    # Outer metallic rim
    rim_grad = QLinearGradient(0, sq_y, 0, sq_y + sq_h)
    rim_grad.setColorAt(0.0, QColor("#444d64"))
    rim_grad.setColorAt(0.4, QColor("#2a3040"))
    rim_grad.setColorAt(1.0, QColor("#141720"))
    painter.setPen(QPen(QBrush(rim_grad), 2.5 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(squircle_path)

    # 3. Inner Squircle Display Canvas (Recessed dark area like in refu.png)
    inner_margin = 38.0 * scale
    inner_sq_rect = QRectF(
        sq_x + inner_margin,
        sq_y + inner_margin,
        sq_w - inner_margin * 2.0,
        sq_h - inner_margin * 2.0,
    )
    inner_sq_r = sq_r - inner_margin * 0.75
    inner_sq_path = QPainterPath()
    inner_sq_path.addRoundedRect(inner_sq_rect, inner_sq_r, inner_sq_r)

    # Deep recessed background fill
    inner_bg_grad = QLinearGradient(0, inner_sq_rect.top(), 0, inner_sq_rect.bottom())
    inner_bg_grad.setColorAt(0.0, QColor("#12141a"))
    inner_bg_grad.setColorAt(1.0, QColor("#0a0b0e"))
    painter.fillPath(inner_sq_path, inner_bg_grad)

    # Inner bezel border
    painter.setPen(QPen(QColor("#151820"), 2.0 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(inner_sq_path)

    # 4. Film Clapperboard Header (Top)
    clap_x = 186.0 * scale
    clap_y = 170.0 * scale
    clap_w = 652.0 * scale
    clap_h = 138.0 * scale
    clap_r = 24.0 * scale
    clap_rect = QRectF(clap_x, clap_y, clap_w, clap_h)

    clap_path = QPainterPath()
    clap_path.addRoundedRect(clap_rect, clap_r, clap_r)

    # Draw clapper content clipped to its rounded container
    painter.save()
    painter.setClipPath(clap_path)

    # Base vibrant Python yellow
    clap_yellow_grad = QLinearGradient(0, clap_y, 0, clap_y + clap_h)
    clap_yellow_grad.setColorAt(0.0, QColor("#ffe057"))
    clap_yellow_grad.setColorAt(1.0, QColor("#ffd43b"))
    painter.fillRect(clap_rect, clap_yellow_grad)

    # Diagonal hazard stripes (tilted ~63 degrees, matching reference clapper)
    dark_stripe = QColor("#141720")
    stripe_w = 44.0 * scale
    stripe_spacing = 82.0 * scale
    slope_dx = 50.0 * scale

    start_x = clap_x - 120.0 * scale
    end_x = clap_x + clap_w + 120.0 * scale

    painter.setPen(Qt.PenStyle.NoPen)
    painter.setBrush(dark_stripe)

    cur_x = start_x
    while cur_x < end_x:
        poly = QPainterPath()
        p1 = QPointF(cur_x, clap_y - 5.0 * scale)
        p2 = QPointF(cur_x + stripe_w, clap_y - 5.0 * scale)
        p3 = QPointF(cur_x + stripe_w + slope_dx, clap_y + clap_h + 5.0 * scale)
        p4 = QPointF(cur_x + slope_dx, clap_y + clap_h + 5.0 * scale)
        poly.moveTo(p1)
        poly.lineTo(p2)
        poly.lineTo(p3)
        poly.lineTo(p4)
        poly.closeSubpath()
        painter.drawPath(poly)
        cur_x += stripe_spacing

    # Subtle top highlight on clapper
    sheen_grad = QLinearGradient(0, clap_y, 0, clap_y + 12.0 * scale)
    sheen_grad.setColorAt(0.0, QColor(255, 255, 255, 60))
    sheen_grad.setColorAt(1.0, QColor(255, 255, 255, 0))
    painter.fillRect(QRectF(clap_x, clap_y, clap_w, 12.0 * scale), sheen_grad)

    painter.restore()

    # Clapper container stroke / border
    painter.setPen(QPen(QColor("#0a0b0e"), 2.0 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(clap_path)

    # 5. Play Button Triangle (Left - Python Blue #306998)
    tri_left_x = 312.0 * scale
    tri_right_x = 446.0 * scale
    tri_top_y = 478.0 * scale
    tri_bot_y = 672.0 * scale
    tri_mid_y = (tri_top_y + tri_bot_y) / 2.0

    tri_path = QPainterPath()
    r = 12.0 * scale

    tri_path.moveTo(tri_left_x + r, tri_top_y)
    tri_path.lineTo(tri_right_x - r * 1.2, tri_mid_y - r * 0.7)
    tri_path.quadTo(
        QPointF(tri_right_x, tri_mid_y),
        QPointF(tri_right_x - r * 1.2, tri_mid_y + r * 0.7),
    )
    tri_path.lineTo(tri_left_x + r, tri_bot_y)
    tri_path.quadTo(
        QPointF(tri_left_x, tri_bot_y),
        QPointF(tri_left_x, tri_bot_y - r),
    )
    tri_path.lineTo(tri_left_x, tri_top_y + r)
    tri_path.quadTo(
        QPointF(tri_left_x, tri_top_y),
        QPointF(tri_left_x + r, tri_top_y),
    )
    tri_path.closeSubpath()

    blue_grad = QLinearGradient(tri_left_x, tri_top_y, tri_right_x, tri_bot_y)
    blue_grad.setColorAt(0.0, QColor("#387ab5"))
    blue_grad.setColorAt(0.5, QColor("#306998"))
    blue_grad.setColorAt(1.0, QColor("#285880"))

    painter.setPen(Qt.PenStyle.NoPen)
    painter.fillPath(tri_path, blue_grad)

    blue_rim = QLinearGradient(tri_left_x, tri_top_y, tri_right_x, tri_bot_y)
    blue_rim.setColorAt(0.0, QColor(255, 255, 255, 60))
    blue_rim.setColorAt(1.0, QColor(0, 0, 0, 30))
    painter.setPen(QPen(QBrush(blue_rim), 1.2 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(tri_path)

    # 6. Center Vertical Dotted Line
    dot_x = 502.0 * scale
    dot_start_y = 478.0 * scale
    dot_end_y = 672.0 * scale
    dot_diameter = 6.5 * scale
    dot_gap = 16.0 * scale

    painter.setPen(Qt.PenStyle.NoPen)
    painter.setBrush(QColor("#333b4d"))
    cur_y = dot_start_y
    while cur_y <= dot_end_y:
        painter.drawEllipse(
            QRectF(dot_x - dot_diameter / 2.0, cur_y, dot_diameter, dot_diameter)
        )
        cur_y += dot_gap

    # 7. Caption Subtitle Pills (Right)
    pill_x = 554.0 * scale

    # Pill 1: Top Yellow Pill (#ffd43b)
    p1_y = 478.0 * scale
    p1_w = 232.0 * scale
    p1_h = 58.0 * scale
    p1_r = p1_h / 2.0
    p1_rect = QRectF(pill_x, p1_y, p1_w, p1_h)
    p1_path = QPainterPath()
    p1_path.addRoundedRect(p1_rect, p1_r, p1_r)

    y_grad1 = QLinearGradient(pill_x, p1_y, pill_x, p1_y + p1_h)
    y_grad1.setColorAt(0.0, QColor("#ffe057"))
    y_grad1.setColorAt(1.0, QColor("#ffd43b"))
    painter.setPen(Qt.PenStyle.NoPen)
    painter.fillPath(p1_path, y_grad1)

    p1_rim = QLinearGradient(pill_x, p1_y, pill_x, p1_y + p1_h)
    p1_rim.setColorAt(0.0, QColor(255, 255, 255, 60))
    p1_rim.setColorAt(1.0, QColor(0, 0, 0, 20))
    painter.setPen(QPen(QBrush(p1_rim), 1.2 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(p1_path)

    # Pill 2: Middle Yellow Pill (#ffd43b)
    p2_y = 552.0 * scale
    p2_w = 172.0 * scale
    p2_h = 58.0 * scale
    p2_r = p2_h / 2.0
    p2_rect = QRectF(pill_x, p2_y, p2_w, p2_h)
    p2_path = QPainterPath()
    p2_path.addRoundedRect(p2_rect, p2_r, p2_r)

    painter.setPen(Qt.PenStyle.NoPen)
    painter.fillPath(p2_path, y_grad1)
    painter.setPen(QPen(QBrush(p1_rim), 1.2 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(p2_path)

    # Pill 3: Bottom Slate Grey Pill (#707e94)
    p3_y = 626.0 * scale
    p3_w = 114.0 * scale
    p3_h = 50.0 * scale
    p3_r = p3_h / 2.0
    p3_rect = QRectF(pill_x, p3_y, p3_w, p3_h)
    p3_path = QPainterPath()
    p3_path.addRoundedRect(p3_rect, p3_r, p3_r)

    grey_grad = QLinearGradient(pill_x, p3_y, pill_x, p3_y + p3_h)
    grey_grad.setColorAt(0.0, QColor("#7b8a9f"))
    grey_grad.setColorAt(1.0, QColor("#707e94"))
    painter.setPen(Qt.PenStyle.NoPen)
    painter.fillPath(p3_path, grey_grad)

    p3_rim = QLinearGradient(pill_x, p3_y, pill_x, p3_y + p3_h)
    p3_rim.setColorAt(0.0, QColor(255, 255, 255, 50))
    p3_rim.setColorAt(1.0, QColor(0, 0, 0, 25))
    painter.setPen(QPen(QBrush(p3_rim), 1.2 * scale))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawPath(p3_path)

    painter.end()
    return img


def generate_svg() -> str:
    return """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <filter id="icon-shadow" x="-15%" y="-10%" width="130%" height="135%" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="16" stdDeviation="24" flood-color="#000000" flood-opacity="0.38"/>
      <feDropShadow dx="0" dy="6" stdDeviation="10" flood-color="#000000" flood-opacity="0.25"/>
    </filter>
    <linearGradient id="frame-bg" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#222734"/>
      <stop offset="50%" stop-color="#191d26"/>
      <stop offset="100%" stop-color="#12141c"/>
    </linearGradient>
    <linearGradient id="frame-rim" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#444d64"/>
      <stop offset="40%" stop-color="#2a3040"/>
      <stop offset="100%" stop-color="#141720"/>
    </linearGradient>
    <linearGradient id="inner-bg" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#12141a"/>
      <stop offset="100%" stop-color="#0a0b0e"/>
    </linearGradient>
    <linearGradient id="clapper-yellow" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#ffe057"/>
      <stop offset="100%" stop-color="#ffd43b"/>
    </linearGradient>
    <linearGradient id="play-blue" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#387ab5"/>
      <stop offset="50%" stop-color="#306998"/>
      <stop offset="100%" stop-color="#285880"/>
    </linearGradient>
    <linearGradient id="play-highlight" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.35"/>
      <stop offset="100%" stop-color="#000000" stop-opacity="0.15"/>
    </linearGradient>
    <linearGradient id="pill-yellow" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#ffe057"/>
      <stop offset="100%" stop-color="#ffd43b"/>
    </linearGradient>
    <linearGradient id="pill-grey" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#7b8a9f"/>
      <stop offset="100%" stop-color="#707e94"/>
    </linearGradient>
    <clipPath id="clapper-clip">
      <rect x="186" y="170" width="652" height="138" rx="24" ry="24"/>
    </clipPath>
  </defs>

  <!-- Outer Frame Squircle with Drop Shadow -->
  <rect x="100" y="80" width="824" height="824" rx="185" ry="185"
        fill="url(#frame-bg)" stroke="url(#frame-rim)" stroke-width="2.5"
        filter="url(#icon-shadow)" />

  <!-- Inner Recessed Display Canvas -->
  <rect x="138" y="118" width="748" height="748" rx="156" ry="156"
        fill="url(#inner-bg)" stroke="#151820" stroke-width="2" />

  <!-- Film Clapperboard Header (Top) -->
  <g clip-path="url(#clapper-clip)">
    <rect x="186" y="170" width="652" height="138" fill="url(#clapper-yellow)"/>
    <!-- Diagonal Hazard Stripes -->
    <path d="
      M 116 165 L 160 165 L 210 313 L 166 313 Z
      M 198 165 L 242 165 L 292 313 L 248 313 Z
      M 280 165 L 324 165 L 374 313 L 330 313 Z
      M 362 165 L 406 165 L 456 313 L 412 313 Z
      M 444 165 L 488 165 L 538 313 L 494 313 Z
      M 526 165 L 570 165 L 620 313 L 576 313 Z
      M 608 165 L 652 165 L 702 313 L 658 313 Z
      M 690 165 L 734 165 L 784 313 L 740 313 Z
      M 772 165 L 816 165 L 866 313 L 822 313 Z
      M 854 165 L 898 165 L 948 313 L 904 313 Z
    " fill="#141720"/>
    <!-- Top Highlight Sheen -->
    <rect x="186" y="170" width="652" height="14" fill="#ffffff" opacity="0.22"/>
  </g>
  <rect x="186" y="170" width="652" height="138" rx="24" ry="24"
        fill="none" stroke="#0a0b0e" stroke-width="2"/>

  <!-- Play Button Triangle (Left - Python Blue) -->
  <path d="M 324 478
           L 434 564
           Q 446 575 434 586
           L 324 672
           Q 312 672 312 660
           L 312 490
           Q 312 478 324 478 Z"
        fill="url(#play-blue)" stroke="url(#play-highlight)" stroke-width="1.2"/>

  <!-- Center Dotted Line -->
  <g fill="#333b4d">
    <circle cx="502" cy="478" r="3.25"/>
    <circle cx="502" cy="494" r="3.25"/>
    <circle cx="502" cy="510" r="3.25"/>
    <circle cx="502" cy="526" r="3.25"/>
    <circle cx="502" cy="542" r="3.25"/>
    <circle cx="502" cy="558" r="3.25"/>
    <circle cx="502" cy="574" r="3.25"/>
    <circle cx="502" cy="590" r="3.25"/>
    <circle cx="502" cy="606" r="3.25"/>
    <circle cx="502" cy="622" r="3.25"/>
    <circle cx="502" cy="638" r="3.25"/>
    <circle cx="502" cy="654" r="3.25"/>
    <circle cx="502" cy="670" r="3.25"/>
  </g>

  <!-- Caption Subtitle Pills (Right) -->
  <!-- Pill 1 (Top Yellow) -->
  <rect x="554" y="478" width="232" height="58" rx="29" ry="29"
        fill="url(#pill-yellow)" stroke="#ffffff" stroke-opacity="0.2" stroke-width="1.2"/>
  <!-- Pill 2 (Middle Yellow) -->
  <rect x="554" y="552" width="172" height="58" rx="29" ry="29"
        fill="url(#pill-yellow)" stroke="#ffffff" stroke-opacity="0.2" stroke-width="1.2"/>
  <!-- Pill 3 (Bottom Slate Grey) -->
  <rect x="554" y="626" width="114" height="50" rx="25" ry="25"
        fill="url(#pill-grey)" stroke="#ffffff" stroke-opacity="0.18" stroke-width="1.2"/>
</svg>
"""


def write_ico_file(png_images: list[tuple[int, bytes]], output_path: Path):
    count = len(png_images)
    header = (
        (0).to_bytes(2, "little")
        + (1).to_bytes(2, "little")
        + count.to_bytes(2, "little")
    )

    entries = bytearray()
    image_data_offset = 6 + (16 * count)
    images_bytes = bytearray()

    for size, data in png_images:
        w = size if size < 256 else 0
        h = size if size < 256 else 0
        b_color = 0
        reserved = 0
        planes = 1
        bpp = 32
        data_len = len(data)

        entry = bytearray()
        entry += w.to_bytes(1, "little")
        entry += h.to_bytes(1, "little")
        entry += b_color.to_bytes(1, "little")
        entry += reserved.to_bytes(1, "little")
        entry += planes.to_bytes(2, "little")
        entry += bpp.to_bytes(2, "little")
        entry += data_len.to_bytes(4, "little")
        entry += image_data_offset.to_bytes(4, "little")

        entries += entry
        image_data_offset += data_len
        images_bytes += data

    with open(output_path, "wb") as f:
        f.write(header + entries + images_bytes)


def main():
    _app = QApplication.instance() or QApplication(sys.argv)

    repo_root = Path(__file__).resolve().parents[1]
    resources_icons_dir = repo_root / "resources" / "icons"
    resources_icons_dir.mkdir(parents=True, exist_ok=True)

    pyside_res_dir = repo_root / "pyside" / "app" / "resources"
    pyside_res_dir.mkdir(parents=True, exist_ok=True)

    electron_res_dir = repo_root / "electron" / "resources" / "build"
    electron_res_dir.mkdir(parents=True, exist_ok=True)

    print("🎨 Rendering master 1024x1024 icon with PySide6 QPainter...")
    master_img = create_master_icon(1024)

    # Save master 1024x1024 PNG
    master_png_path = resources_icons_dir / "icon_1024x1024.png"
    master_img.save(str(master_png_path), "PNG")
    master_img.save(str(resources_icons_dir / "icon.png"), "PNG")
    master_img.save(str(pyside_res_dir / "icon.png"), "PNG")
    master_img.save(str(electron_res_dir / "icon.png"), "PNG")
    print(f"  ✓ Saved {master_png_path}")

    # Standard sizes: 16, 24, 32, 48, 64, 128, 256, 512, 1024
    sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    ico_png_data = []

    iconset_dir = resources_icons_dir / "icon.iconset"
    iconset_dir.mkdir(parents=True, exist_ok=True)

    for sz in sizes:
        scaled = master_img.scaled(
            sz,
            sz,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        dest = resources_icons_dir / f"icon_{sz}x{sz}.png"
        scaled.save(str(dest), "PNG")
        print(f"  ✓ Saved {dest.name}")

        if sz in (16, 24, 32, 48, 64, 128, 256):
            buf = QBuffer()
            buf.open(QIODevice.OpenModeFlag.WriteOnly)
            scaled.save(buf, "PNG")
            ico_png_data.append((sz, bytes(buf.data())))

    iconutil_map = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ]

    for name, sz in iconutil_map:
        scaled = master_img.scaled(
            sz,
            sz,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        scaled.save(str(iconset_dir / name), "PNG")

    icns_path = resources_icons_dir / "icon.icns"
    if os.path.exists("/usr/bin/iconutil"):
        print("🍏 Building macOS icon.icns with iconutil...")
        try:
            subprocess.run(
                [
                    "/usr/bin/iconutil",
                    "-c",
                    "icns",
                    str(iconset_dir),
                    "-o",
                    str(icns_path),
                ],
                check=True,
            )
            shutil.copy2(icns_path, electron_res_dir / "icon.icns")
            shutil.copy2(icns_path, pyside_res_dir / "icon.icns")
            print(f"  ✓ Built native {icns_path}")
        except subprocess.CalledProcessError as e:
            print(f"  ⚠ iconutil failed: {e}")

    ico_path = resources_icons_dir / "icon.ico"
    print("🪟 Building Windows icon.ico...")
    write_ico_file(ico_png_data, ico_path)
    shutil.copy2(ico_path, electron_res_dir / "icon.ico")
    shutil.copy2(ico_path, pyside_res_dir / "icon.ico")
    print(f"  ✓ Built {ico_path}")

    svg_content = generate_svg()
    svg_path = resources_icons_dir / "icon.svg"
    svg_path.write_text(svg_content, encoding="utf-8")
    shutil.copy2(svg_path, electron_res_dir / "icon.svg")
    shutil.copy2(svg_path, pyside_res_dir / "icon.svg")
    print(f"  ✓ Built {svg_path}")

    print("\n🎉 All app icons successfully generated!")


if __name__ == "__main__":
    main()
