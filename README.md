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

Build and install with [just](https://github.com/casey/just):

```sh
just && sudo just install
```

This installs the binary to `/usr/bin/snappea` and adds a desktop entry to your application menu.

**Setting up a keyboard shortcut:**

1. Open **Settings** > **Keyboard** > **Keyboard Shortcuts** > **Custom Shortcuts**
2. Add a new shortcut:
   - Name: `SnapPea` (or `Screenshot`)
   - Command: `snappea`
   - Shortcut: `Print` (or your preferred key)

> [!NOTE]
> Running `snappea` multiple times communicates with the existing instance. If you're recording and press the shortcut again, it will stop the recording.

### Optional: System Integration

To use SnapPea as the default screenshot tool (replacing the COSMIC screenshot tool):

```sh
just install-portal
```

This creates `~/.config/xdg-desktop-portal/portals.conf` to configure xdg-desktop-portal.

After installing, reload the portal:

```bash
systemctl --user restart xdg-desktop-portal
```

Now pressing `Print Screen` will open SnapPea instead of the default screenshot tool.

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

The uninstall command will warn you if `~/.config/xdg-desktop-portal/portals.conf` still contains SnapPea configuration.

## Why SnapPea?
It Snaps Pics and it's snappy!
