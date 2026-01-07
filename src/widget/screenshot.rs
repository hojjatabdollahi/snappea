//! Widget for displaying a selected window with a border
//! 
//! The main `ScreenshotSelectionWidget` has been moved to `screenshot_selection/widget.rs`.
//! This file contains `SelectedImageWidget` used for window mode display.

use std::collections::HashMap;

use cosmic::{
    Element,
    iced_core::{Background, Border, Layout, Length, Size, layout, widget::Tree},
};

use crate::capture::image::ScreenshotImage;

/// Widget for displaying a selected window with a border (buttons are in toolbar)
pub struct SelectedImageWidget {
    image_handle: Option<cosmic::widget::image::Handle>,
    image_size: (u32, u32),
}

impl SelectedImageWidget {
    pub fn new(
        output_name: String,
        window_index: Option<usize>,
        toplevel_images: &HashMap<String, Vec<ScreenshotImage>>,
        screen_size: (u32, u32),
    ) -> Self {
        let (image_handle, image_size) = if let Some(window_index) = window_index {
            toplevel_images
                .get(&output_name)
                .and_then(|imgs| imgs.get(window_index))
                .map(|img| {
                    let orig_width = img.rgba.width();
                    let orig_height = img.rgba.height();

                    // Use 85% of screen size as the maximum preview size
                    let max_width = (screen_size.0 as f32 * 0.85) as u32;
                    let max_height = (screen_size.1 as f32 * 0.85) as u32;

                    // If the image is larger than the screen, create a scaled thumbnail
                    // using high-quality Lanczos3 filtering for better downscaling
                    if orig_width > max_width || orig_height > max_height {
                        let scale = (max_width as f32 / orig_width as f32)
                            .min(max_height as f32 / orig_height as f32);
                        let new_width = (orig_width as f32 * scale) as u32;
                        let new_height = (orig_height as f32 * scale) as u32;

                        let scaled = ::image::imageops::resize(
                            &img.rgba,
                            new_width,
                            new_height,
                            ::image::imageops::FilterType::Lanczos3,
                        );

                        let handle = cosmic::widget::image::Handle::from_rgba(
                            new_width,
                            new_height,
                            scaled.into_vec(),
                        );
                        (Some(handle), (new_width, new_height))
                    } else {
                        // Image fits on screen - use original at 1:1
                        (Some(img.handle.clone()), (orig_width, orig_height))
                    }
                })
                .unwrap_or((None, (0, 0)))
        } else {
            (None, (0, 0))
        };

        Self {
            image_handle,
            image_size,
        }
    }

    /// Calculate the bounds where the image should be drawn (centered in the output)
    pub fn image_bounds(
        &self,
        layout_bounds: cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::Rectangle {
        if self.image_handle.is_some() && self.image_size.0 > 0 {
            let img_width = self.image_size.0 as f32;
            let img_height = self.image_size.1 as f32;

            // Leave small margin around the image
            let available_width = layout_bounds.width - 20.0;
            let available_height = layout_bounds.height - 20.0;

            // Calculate scale to fit image within available space
            let scale_x = available_width / img_width;
            let scale_y = available_height / img_height;
            let scale = scale_x.min(scale_y).min(1.0); // Don't upscale

            let display_width = img_width * scale;
            let display_height = img_height * scale;

            cosmic::iced_core::Rectangle {
                x: layout_bounds.x + (layout_bounds.width - display_width) / 2.0,
                y: layout_bounds.y + (layout_bounds.height - display_height) / 2.0,
                width: display_width,
                height: display_height,
            }
        } else {
            layout_bounds
        }
    }
}

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for SelectedImageWidget
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        layout::Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        _cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;

        let cosmic_theme = theme.cosmic();
        let layout_bounds = layout.bounds();
        let image_bounds = self.image_bounds(layout_bounds);

        // Draw dark overlay outside the selected window (60% opacity, matching region selection)
        let overlay_color = Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6));

        // Top strip
        if image_bounds.y > layout_bounds.y {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: layout_bounds.y,
                        width: layout_bounds.width,
                        height: image_bounds.y - layout_bounds.y,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Bottom strip
        let image_bottom = image_bounds.y + image_bounds.height;
        let layout_bottom = layout_bounds.y + layout_bounds.height;
        if image_bottom < layout_bottom {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: image_bottom,
                        width: layout_bounds.width,
                        height: layout_bottom - image_bottom,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Left strip (between top and bottom)
        if image_bounds.x > layout_bounds.x {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: image_bounds.y,
                        width: image_bounds.x - layout_bounds.x,
                        height: image_bounds.height,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Right strip (between top and bottom)
        let image_right = image_bounds.x + image_bounds.width;
        let layout_right = layout_bounds.x + layout_bounds.width;
        if image_right < layout_right {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: image_right,
                        y: image_bounds.y,
                        width: layout_right - image_right,
                        height: image_bounds.height,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Draw the image with linear filtering for better quality when scaling down
        if let Some(ref handle) = self.image_handle {
            cosmic::iced_core::image::Renderer::draw_image(
                renderer,
                handle.clone(),
                cosmic::iced_core::image::FilterMethod::Linear,
                image_bounds,
                cosmic::iced::Radians(0.0),
                1.0,
                [0.0, 0.0, 0.0, 0.0],
            );
        }

        // Draw border around the image
        let accent = cosmic::iced::Color::from(cosmic_theme.accent_color());

        // Semi-transparent glow
        let mut glow_color = accent;
        glow_color.a = 0.5;
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: image_bounds.x - 2.0,
                    y: image_bounds.y - 2.0,
                    width: image_bounds.width + 4.0,
                    height: image_bounds.height + 4.0,
                },
                border: Border {
                    radius: 0.0.into(),
                    width: 6.0,
                    color: glow_color,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(cosmic::iced::Color::TRANSPARENT),
        );

        // Solid accent border
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: image_bounds,
                border: Border {
                    radius: 0.0.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(cosmic::iced::Color::TRANSPARENT),
        );

        // Corner handles
        let corner_size = 12.0;
        let corners = [
            (image_bounds.x, image_bounds.y),
            (image_bounds.x + image_bounds.width, image_bounds.y),
            (image_bounds.x, image_bounds.y + image_bounds.height),
            (
                image_bounds.x + image_bounds.width,
                image_bounds.y + image_bounds.height,
            ),
        ];
        for (cx, cy) in corners {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: cx - corner_size / 2.0,
                        y: cy - corner_size / 2.0,
                        width: corner_size,
                        height: corner_size,
                    },
                    border: Border {
                        radius: cosmic_theme.radius_s().into(),
                        width: 0.0,
                        color: cosmic::iced::Color::TRANSPARENT,
                    },
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(accent),
            );
        }
    }
}

impl<'a, Msg: Clone + 'static> From<SelectedImageWidget> for Element<'a, Msg> {
    fn from(widget: SelectedImageWidget) -> Self {
        Element::new(widget)
    }
}
