<p align="center">
  <img src="snappea/data/logo.svg" alt="SnapPea Logo" width="128">
</p>

# SnapPea

A screenshot and screen recording tool for the COSMIC desktop environment with annotation capabilities.





Screenshot Demo

https://github.com/user-attachments/assets/2db24b53-25ca-4890-b2b7-ba115880df75

Screencast Demo

https://github.com/user-attachments/assets/bf64b613-d068-46be-be07-3fec3e95534b


Disclaimer: This project is based on xdg-desktop-portal-cosmic with added features. It implements the same screenshot portal interface. When set up correctly, dbus messages sent by `cosmic-screenshot` will be handled by SnapPea instead of `xdg-desktop-portal-cosmic` (only Screenshot Portal messages).

## Features

- Interactive screenshot selection
- Screen recording with hardware acceleration
  - Multiple container formats (MP4, WebM, MKV)
  - Configurable framerate (24/30/60 fps)
  - Hardware encoder selection
  - Cursor visibility toggle
  - Live annotations while recording
  - System audio and microphone audio recording
- Video editor
  - Trim recorded video
  - Save as gif
  - Save as WebM
  - Remove audio
- Annotation tools: pixelate, arrow, shapes, pencil
- Text recognition (OCR)
- QR code detection

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


**Setting up a custom keyboard shortcut:**

1. Open **Settings** > **Keyboard** > **Keyboard Shortcuts** > **Custom Shortcuts**
2. Add a new shortcut:
   - Name: `SnapPea` (or `Screenshot`)
   - Command: `snappea`
   - Shortcut: `Print` (or your preferred key)

> [!NOTE]
> Running `snappea` multiple times communicates with the existing instance. If you're recording and press the shortcut again, it will stop the recording.

### Optional: Set as Default Screenshot Tool

To use SnapPea as the default screenshot portal (replacing the COSMIC screenshot tool when `Print Screen` is pressed):

1. Open SnapPea by running it from cosmic-launcher (super key) or cosmic-app-library (super+a)
2. Click the **Settings** icon
3. Enable **Default screenshot app**

This automatically registers SnapPea as the preferred screenshot portal for your user account and restarts `xdg-desktop-portal` to apply the change immediately. No root access or manual config editing required.

### Optional: Better gif quality
To get better gif quality install [gifski](https://gif.ski/).

### Optional: OCR Support

To enable text recognition (OCR), install [tesseract-ocr](https://github.com/tesseract-ocr/tesseract):

```bash
# Debian/Ubuntu
sudo apt install tesseract-ocr

# Fedora
sudo dnf install tesseract

```

### Uninstalling

Remove snappea as the default screenshot tool and run `sudo apt remove snappea` or `sudo just uninstall` if you installed it from source.

The uninstall command will warn you if `~/.config/xdg-desktop-portal/cosmic-portals.conf` still contains SnapPea configuration.

## Why SnapPea?
It Snaps Pics and it's snappy!
