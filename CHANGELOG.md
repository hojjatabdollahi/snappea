# Changelog

All notable changes to SnapPea are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.3.0] - 2026-10-30

### Added

- Text annotation now can have a custom size and a backdrop.
- Shapes and pixelate can be edited after drawing.
- You can select a window instead of drawing a region around it, by just clicking on it. `click_selects_window: false` in the config file turns that off.
- Add a toggle to edit videos after recording.
- The magnifier shows a separate circle for the source, its handles sit on the corners, and the zoom level can be changed after drawing.
- Recording refuses to start/stops, when the save folder has less free space than `video_min_free_mb`.

### Changed

- Default name for the output files is now just the date, with no prefix or suffixes.
- The toolbar starts a little higher above the bottom edge.
- Show file name in the video editor header instead of the full path.
- Use libcosmic's dropdown widget for the video speed dropdown
- Holding Ctrl constrains a shape while reshaping it too.

### Fixed

- Annotations in "All Screens" mode are now saved.
- Toolbar popups open away from the nearest screen edge.
- A press outside an open toolbar dropdown now only closes it
- Drawing on one screen no longer shows a stray piece of the shape on another output
- A color from the custom picker now applies to the selected annotation
- An annotation that sticks out of the region can be reshaped by the handle outside it.
- A click without drag doesn't draw a shape, or pixelate, or magnifier annotation.
- The toolbar changed width when the text tool was armed or disarmed, since the font button was narrower than the size dropdown it replaces.
- Changing the picked screen with S then clicking another could pick a third one, the click now names the screen under it.
- A window closed just before a screenshot could still be picked by a click, the list kept it until the next window event.
- On a fresh start the overlay could open with no windows to click, it no longer waits on a guess, and takes the list once the compositor has sent it.
- Pointer shapes are now more consistent and signal the possible action better.
- A screenshot that could not be written to disk, failed silently and left an empty file behind. The file is now removed and a notification is shown.
- Sometimes when drawing a region, the selection box starts from some random point.
- Old text stayed visible while resizing or editing a text label.
- The editor showed a mute button and passed audio flags to the export for recordings without an audio track.
- Exporting with a VA-API encoder failed silently when the video had audio, since the quality flag also reached the audio encoder.

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
