# Changelog

All notable changes to SnapPea are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added

- A background behind text labels
- Text labels start centered, horizontally and vertically, and keep whatever box height you drag out. Typing more only ever grows the box. Resizing no longer changes the font size.
- A click without a drag selects the window under it. `click_selects_window: false` in the config file turns that off.
- Add a toggle to edit videos after recording
- Can select pixelate after drawing one
- Recording refuses to start/stops, when the save folder has less free space than `video_min_free_mb`

### Changed

- Show file name in the video editor header instead of the full path.
- Default name for the output files is now just the date, with no prefix or suffixes.
- Use libcosmic's dropdown widget for the video speed dropdown

### Fixed

- A window closed just before a screenshot could still be picked by a click, the list kept it until the next window event.
- On a fresh start the overlay could open with no windows to click, it no longer waits on a guess, and takes the list once the compositor has sent it.
- A screenshot that could not be written to disk, failed silently and left an empty file behind. The file is now removed and a notification is shown.
- Annotations in "All Screens" mode are now saved.
- Sometimes when drawing a region, the selection box starts from some random point

## [1.2.0] - 2026-10-23

### Added

- Audio recording for system audio and the microphone.
- A mute button with a live level meter in the recording toolbar.
- The editor shows the audio track in the scrubber.
- Default save folders are now Screenshots and Screencasts.

### Fixed

- Exports from the editor kept dropping the audio track.
- The microphone fades in at the start of a recording.
- When the Wayland connection is lost, the portal exits too.

## [1.1.0] - 2026-10-19

### Added

- A toggle to disable the recording region border.

### Changed

- Enter is the capture key.
- The recording border is not drawn over the screen edges.

### Fixed

- Enter started a delayed capture even when a delay was set.
- The capture delay no longer defaults to 3 seconds.

## [1.0.0] - 2026-10-13

### Changed

- Redesigned interface (#23).
- Follows the screenshot portal and returns the screenshot as a response.
- Fewer redraws when nothing is being drawn.

## [0.9.1] - 2026-09-03

- Initial release.
