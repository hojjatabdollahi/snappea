<p align="center">
  <img src="data/logo.svg" alt="SnapPea Logo" width="128">
</p>

# SnapPea

A screenshot and screen recording tool for the COSMIC desktop environment with annotation capabilities.

<p align="center">
  <video src="data/demo.mp4" width="100%" autoplay loop muted playsinline>
    Your browser does not support the video tag.
  </video>
</p>

Disclaimer: This project is based on xdg-desktop-portal-cosmic with added features. It implements the same screenshot portal interface. When set up correctly, dbus messages sent by `cosmic-screenshot` will be handled by SnapPea instead of `xdg-desktop-portal-cosmic` (only Screenshot Portal messages).

## Features

- Interactive screenshot selection
- Screen recording with hardware acceleration
  - Multiple container formats (MP4, WebM, MKV)
  - Configurable framerate (24/30/60 fps)
  - Hardware encoder selection
  - Cursor visibility toggle
  - Live annotations while recording
- Annotation tools: arrows, circles, squares, freehand drawing
- Text recognition (OCR)
- QR code detection
- Redaction and pixelation
- Multi-window and multi-output support
- Keyboard shortcuts
- Configurable settings

## Installation

Build and install with [just](https://github.com/casey/just):

```sh
just
sudo just install
```


> [!IMPORTANT]
To override the default screenshot portal, create or edit `~/.config/xdg-desktop-portal/portals.conf`:

```ini
[preferred]
org.freedesktop.impl.portal.Screenshot=snappea
```

Reload xdg-desktop-portal to apply changes:

```bash
systemctl --user restart xdg-desktop-portal
```


To enable OCR install [tesseract-ocr](https://github.com/tesseract-ocr/tesseract) and the desired language packs.

```bash
sudo apt install tesseract-ocr
```


To uninstall:

```sh
sudo just uninstall
```

Make sure to remove the override from `~/.config/xdg-desktop-portal/portals.conf` if you set it.

## Why SnapPea?
It Snaps Pics and it's snappy!
