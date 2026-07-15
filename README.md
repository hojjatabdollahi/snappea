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
- Video editor
  - Trim recorded video
  - Save as gif
  - Save as WebM
- Annotation tools: arrows, circles, squares, freehand drawing
- Text recognition (OCR)
- QR code detection
- Redaction and pixelation
- Multi-window and multi-output support
- Keyboard shortcuts
- Configurable settings

## Installation

### From the Cloudsmith Debian Repository (recommended)

[![Cloudsmith](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fapi.cloudsmith.io%2Fv1%2Fpackages%2Fcosmetics%2Fsnappea%2F%3Fpage%3D1%26page_size%3D1%26sort%3D-version&query=%24%5B0%5D.version&label=cloudsmith&logo=cloudsmith&color=blue)](https://cloudsmith.io/~cosmetics/repos/snappea/packages/) <img alt="Static Badge" src="https://img.shields.io/badge/OSS%20hosting%20by-cloudsmith-blue?logo=cloudsmith&style=flat-square&link=https%3A%2F%2Fcloudsmith.com"> </img>

SnapPea repository hosting is graciously provided by [Cloudsmith](https://cloudsmith.com).

For Ubuntu/Pop!_OS 24.04 (Noble):

```sh
# Add the Cloudsmith repository
curl -1sLf 'https://dl.cloudsmith.io/public/cosmetics/snappea/setup.deb.sh' | sudo -E bash

# Install SnapPea
sudo apt install snappea
```

Packages are built automatically on every release and include the binary and desktop entry.

### From Fedora COPR

[![Copr](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fcopr.fedorainfracloud.org%2Fapi_3%2Fpackage%3Fownername%3Dkordus%26projectname%3Dcosmic-apps%26packagename%3Dsnappea%26with_latest_build%3DTrue&query=%24.builds.latest.source_package.version&label=copr&logo=fedora&color=blue)](https://copr.fedorainfracloud.org/coprs/kordus/cosmic-apps/package/snappea/)

COPR packages are maintained by [@lorduskordus](https://github.com/lorduskordus).

**Traditional Fedora:**

```sh
sudo dnf copr enable kordus/cosmic-apps
sudo dnf install snappea
```

**Fedora Atomic:**

```sh
sudo wget \
    https://copr.fedorainfracloud.org/coprs/kordus/cosmic-apps/repo/fedora/kordus-cosmic-apps.repo \
    -O /etc/yum.repos.d/_copr:copr.fedorainfracloud.org:kordus:cosmic-apps.repo
rpm-ostree install snappea
```

### From Source

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

### Optional: Set as Default Screenshot Tool

To use SnapPea as the default screenshot portal (replacing the COSMIC screenshot tool when `Print Screen` is pressed):

1. Open SnapPea
2. Click the **Settings** gear icon
3. Under the **General** tab, enable **Set as Default**

This automatically registers SnapPea as the preferred screenshot portal for your user account and restarts `xdg-desktop-portal` to apply the change immediately. No root access or manual config editing required.

> [!IMPORTANT]
> SnapPea used to install its config to `~/.config/xdg-desktop-portal/portals.conf`. This breaks theming in Flatpaks. If you had an older version of SnapPea, make sure `portals.conf` is deleted.

### Optional: Better gif quality
To get better gif quality install [gifski](https://gif.ski/).

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

The uninstall command will warn you if `~/.config/xdg-desktop-portal/cosmic-portals.conf` still contains SnapPea configuration.

## Why SnapPea?
It Snaps Pics and it's snappy!
