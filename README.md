<p align="center">
  <img src="data/logo.svg" alt="SnapPea Logo" width="128">
</p>

# SnapPea

A screenshot and screen recording tool for the COSMIC desktop environment with annotation capabilities.



https://github.com/user-attachments/assets/8091f7e3-5ecb-4111-abaf-0fe365aa1e1d


Disclaimer: This project is based on xdg-desktop-portal-cosmic with added features. It implements the same screenshot portal interface. When set up correctly, dbus messages sent by `cosmic-screenshot` will be handled by SnapPea instead of `xdg-desktop-portal-cosmic` (only Screenshot Portal messages).

## Features

- Interactive screenshot selection
- Screen recording with hardware acceleration (no audio yet)
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

Build with [just](https://github.com/casey/just):

```sh
just
```

### Option 1: Standalone Mode (Recommended)

Install SnapPea as a standalone application:

```sh
sudo just install
```

This installs the binary to `/usr/bin/snappea` and adds a desktop entry to your application menu.

**Setting up a keyboard shortcut:**

1. Open **Settings** > **Keyboard** > **Keyboard Shortcuts** > **Custom Shortcuts**
2. Add a new shortcut:
   - Name: `SnapPea` (or `Screenshot`)
   - Command: `snappea`
   - Shortcut: `Print` (or your preferred key)

> [!NOTE]
> In standalone mode, running `snappea` multiple times communicates with the existing instance. If you're recording and press the shortcut again, it will stop the recording.

### Option 2: Portal Mode (System Integration)

Install SnapPea as a screenshot portal to replace the default COSMIC screenshot tool:

```sh
sudo just install-portal
```

Then configure xdg-desktop-portal to use SnapPea. Create or edit `~/.config/xdg-desktop-portal/portals.conf`:

```ini
[preferred]
org.freedesktop.impl.portal.Screenshot=snappea
```

Reload xdg-desktop-portal to apply changes:

```bash
systemctl --user restart xdg-desktop-portal
```

Now pressing `Print Screen` will open SnapPea instead of the default screenshot tool.

> [!NOTE]
> Portal mode runs SnapPea as a D-Bus service. The system automatically launches it when you press Print Screen.

### Optional: OCR Support

To enable text recognition (OCR), install [tesseract-ocr](https://github.com/tesseract-ocr/tesseract):

```bash
# Debian/Ubuntu
sudo apt install tesseract-ocr

# Fedora
sudo dnf install tesseract

# Arch
sudo pacman -S tesseract
```

### Uninstalling

```sh
sudo just uninstall
```

If you used portal mode, also remove the override from `~/.config/xdg-desktop-portal/portals.conf`.

## Why SnapPea?
It Snaps Pics and it's snappy!
