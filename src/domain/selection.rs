//! Selection types for screenshot modes

use super::geometry::Rect;

/// Drag state for rectangle selection handles
#[repr(u8)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    #[default]
    None,
    /// North-West corner
    NW,
    /// North edge
    N,
    /// North-East corner
    NE,
    /// East edge
    E,
    /// South-East corner
    SE,
    /// South edge
    S,
    /// South-West corner
    SW,
    /// West edge
    W,
}

impl From<u8> for DragState {
    fn from(state: u8) -> Self {
        match state {
            0 => DragState::None,
            1 => DragState::NW,
            2 => DragState::N,
            3 => DragState::NE,
            4 => DragState::E,
            5 => DragState::SE,
            6 => DragState::S,
            7 => DragState::SW,
            8 => DragState::W,
            _ => DragState::None,
        }
    }
}

impl From<DragState> for u8 {
    fn from(state: DragState) -> Self {
        match state {
            DragState::None => 0,
            DragState::NW => 1,
            DragState::N => 2,
            DragState::NE => 3,
            DragState::E => 4,
            DragState::SE => 5,
            DragState::S => 6,
            DragState::SW => 7,
            DragState::W => 8,
        }
    }
}

/// Selection mode choice
#[derive(Debug, Clone)]
pub enum Choice {
    /// Output selection: None = picker mode (selecting), Some = confirmed (screen locked in)
    Output(Option<String>),
    /// Rectangle selection with current rect and drag state
    Rectangle(Rect, DragState),
    /// Window selection: output name and optional window index
    Window(String, Option<usize>),
}

/// Action to perform after screenshot capture
#[derive(Debug, Clone, Default)]
pub enum Action {
    /// Return the path to the screenshot file (portal default)
    #[default]
    ReturnPath,
    /// Save to clipboard
    SaveToClipboard,
}

/// Where to save the screenshot image
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSaveLocation {
    /// Copy to clipboard only
    Clipboard,
    /// Save to Pictures folder
    #[default]
    Pictures,
    /// Save to Documents folder
    Documents,
}
