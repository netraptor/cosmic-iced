//! A [`wgpu`] renderer for [Iced].
//!
//! ![The native path of the Iced ecosystem](https://github.com/iced-rs/iced/blob/0525d76ff94e828b7b21634fa94a747022001c83/docs/graphs/native.png?raw=true)
//!
//! [`wgpu`] supports most modern graphics backends: Vulkan, Metal, DX11, and
//! DX12 (OpenGL and WebGL are still WIP). Additionally, it will support the
//! incoming [WebGPU API].
//!
//! Currently, `iced_wgpu` supports the following primitives:
//! - Text, which is rendered using [`glyphon`].
//! - Quads or rectangles, with rounded borders and a solid background color.
//! - Clip areas, useful to implement scrollables or hide overflowing content.
//! - Images and SVG, loaded from memory or the file system.
//! - Meshes of triangles, useful to draw geometry freely.
//!
//! [Iced]: https://github.com/iced-rs/iced
//! [`wgpu`]: https://github.com/gfx-rs/wgpu-rs
//! [WebGPU API]: https://gpuweb.github.io/gpuweb/
//! [`glyphon`]: https://github.com/grovesNL/glyphon
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/iced-rs/iced/9ab6923e943f784985e9ef9ca28b10278297225d/docs/logo.svg"
)]
#![forbid(rust_2018_idioms)]
#![deny(
    // missing_debug_implementations,
    //missing_docs,
    unsafe_code,
    unused_results,
    rustdoc::broken_intra_doc_links
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
pub mod layer;
pub mod primitive;
pub mod settings;
pub mod window;

#[cfg(feature = "geometry")]
pub mod geometry;

mod buffer;
mod color;
mod engine;
mod quad;
mod text;
mod triangle;

#[cfg(any(feature = "image", feature = "svg"))]
#[path = "image/mod.rs"]
mod image;

#[cfg(not(any(feature = "image", feature = "svg")))]
#[path = "image/null.rs"]
mod image;

use buffer::Buffer;

pub use iced_graphics as graphics;
pub use iced_graphics::core;

pub use wgpu;

pub use engine::Engine;
pub use layer::{Layer, LayerMut};
pub use primitive::Primitive;
pub use settings::Settings;

#[cfg(feature = "geometry")]
pub use geometry::Geometry;

use crate::core::{
    Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation,
};
use crate::graphics::text::{Editor, Paragraph};
use crate::graphics::Viewport;

use std::borrow::Cow;

/// A [`wgpu`] graphics renderer for [`iced`].
///
/// [`wgpu`]: https://github.com/gfx-rs/wgpu-rs
/// [`iced`]: https://github.com/iced-rs/iced
#[allow(missing_debug_implementations)]
pub struct Renderer {
    default_font: Font,
    default_text_size: Pixels,
    layers: layer::Stack,

    // TODO: Centralize all the image feature handling
    #[cfg(any(feature = "svg", feature = "image"))]
    image_cache: image::cache::Shared,
}

impl Renderer {
    pub fn new(settings: Settings, _engine: &Engine) -> Self {
        Self {
            default_font: settings.default_font,
            default_text_size: settings.default_text_size,
            layers: layer::Stack::new(),

            #[cfg(any(feature = "svg", feature = "image"))]
            image_cache: _engine.image_cache().clone(),
        }
    }

    pub fn draw_primitive(&mut self, _primitive: Primitive) {}

    pub fn present<T: AsRef<str>>(
        &mut self,
        engine: &mut Engine,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: Option<Color>,
        format: wgpu::TextureFormat,
        frame: &wgpu::TextureView,
        viewport: &Viewport,
        overlay: &[T],
    ) {
        let target_size = viewport.physical_size();
        let scale_factor = viewport.scale_factor() as f32;
        let transformation = viewport.projection();

        for line in overlay {
            println!("{}", line.as_ref());
        }

        self.prepare(
            engine,
            device,
            queue,
            format,
            encoder,
            scale_factor,
            target_size,
            transformation,
        );

        self.render(
            engine,
            device,
            encoder,
            frame,
            clear_color,
            scale_factor,
            target_size,
        );
    }

    fn prepare(
        &mut self,
        engine: &mut Engine,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        scale_factor: f32,
        target_size: Size<u32>,
        transformation: Transformation,
    ) {
        for layer in self.layers.iter_mut() {
            match layer {
                LayerMut::Live(live) => {
                    if !live.quads.is_empty() {
                        engine.quad_pipeline.prepare_batch(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &live.quads,
                            transformation,
                            scale_factor,
                        );
                    }

                    if !live.meshes.is_empty() {
                        engine.triangle_pipeline.prepare_batch(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &live.meshes,
                            transformation
                                * Transformation::scale(scale_factor),
                        );
                    }

                    if !live.text.is_empty() {
                        engine.text_pipeline.prepare_batch(
                            device,
                            queue,
                            encoder,
                            &live.text,
                            live.bounds.unwrap_or(Rectangle::with_size(
                                Size::INFINITY,
                            )),
                            scale_factor,
                            target_size,
                        );
                    }

                    #[cfg(any(feature = "svg", feature = "image"))]
                    if !live.images.is_empty() {
                        engine.image_pipeline.prepare(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &live.images,
                            transformation,
                            scale_factor,
                        );
                    }
                }
                LayerMut::Cached(mut cached) => {
                    if !cached.quads.is_empty() {
                        engine.quad_pipeline.prepare_cache(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &mut cached.quads,
                            transformation,
                            scale_factor,
                        );
                    }

                    if !cached.meshes.is_empty() {
                        engine.triangle_pipeline.prepare_cache(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &mut cached.meshes,
                            transformation
                                * Transformation::scale(scale_factor),
                        );
                    }

                    if !cached.text.is_empty() {
                        let bounds = cached
                            .bounds
                            .unwrap_or(Rectangle::with_size(Size::INFINITY));

                        engine.text_pipeline.prepare_cache(
                            device,
                            queue,
                            encoder,
                            &mut cached.text,
                            bounds,
                            scale_factor,
                            target_size,
                        );
                    }

                    #[cfg(any(feature = "svg", feature = "image"))]
                    if !cached.images.is_empty() {
                        engine.image_pipeline.prepare(
                            device,
                            encoder,
                            &mut engine.staging_belt,
                            &cached.images,
                            transformation,
                            scale_factor,
                        );
                    }
                }
            }
        }
    }

    fn render(
        &mut self,
        engine: &mut Engine,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        clear_color: Option<Color>,
        scale_factor: f32,
        target_size: Size<u32>,
    ) {
        use std::mem::ManuallyDrop;

        let mut render_pass = ManuallyDrop::new(encoder.begin_render_pass(
            &wgpu::RenderPassDescriptor {
                label: Some("iced_wgpu render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match clear_color {
                            Some(background_color) => wgpu::LoadOp::Clear({
                                let [r, g, b, a] =
                                    graphics::color::pack(background_color)
                                        .components();

                                wgpu::Color {
                                    r: f64::from(r),
                                    g: f64::from(g),
                                    b: f64::from(b),
                                    a: f64::from(a),
                                }
                            }),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            },
        ));

        let mut quad_layer = 0;
        let mut mesh_layer = 0;
        let mut text_layer = 0;

        #[cfg(any(feature = "svg", feature = "image"))]
        let mut image_layer = 0;

        // TODO: Can we avoid collecting here?
        let layers: Vec<_> = self.layers.iter().collect();

        for layer in &layers {
            match layer {
                Layer::Live(live) => {
                    let bounds = live
                        .bounds
                        .map(|bounds| bounds * scale_factor)
                        .map(Rectangle::snap)
                        .unwrap_or(Rectangle::with_size(target_size));

                    if !live.quads.is_empty() {
                        engine.quad_pipeline.render_batch(
                            quad_layer,
                            bounds,
                            &live.quads,
                            &mut render_pass,
                        );

                        quad_layer += 1;
                    }

                    if !live.meshes.is_empty() {
                        let _ = ManuallyDrop::into_inner(render_pass);

                        engine.triangle_pipeline.render_batch(
                            device,
                            encoder,
                            frame,
                            mesh_layer,
                            target_size,
                            &live.meshes,
                            bounds,
                            scale_factor,
                        );

                        mesh_layer += 1;

                        render_pass =
                            ManuallyDrop::new(encoder.begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("iced_wgpu render pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: frame,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Load,
                                                store: wgpu::StoreOp::Store,
                                            },
                                        },
                                    )],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                },
                            ));
                    }

                    if !live.text.is_empty() {
                        engine.text_pipeline.render_batch(
                            text_layer,
                            bounds,
                            &mut render_pass,
                        );

                        text_layer += 1;
                    }

                    #[cfg(any(feature = "svg", feature = "image"))]
                    if !live.images.is_empty() {
                        engine.image_pipeline.render(
                            image_layer,
                            bounds,
                            &mut render_pass,
                        );

                        image_layer += 1;
                    }
                }
                Layer::Cached(cached) => {
                    let bounds = cached
                        .bounds
                        .map(|bounds| bounds * scale_factor)
                        .map(Rectangle::snap)
                        .unwrap_or(Rectangle::with_size(target_size));

                    if !cached.quads.is_empty() {
                        engine.quad_pipeline.render_cache(
                            &cached.quads,
                            bounds,
                            &mut render_pass,
                        );
                    }

                    if !cached.meshes.is_empty() {
                        let _ = ManuallyDrop::into_inner(render_pass);

                        engine.triangle_pipeline.render_cache(
                            device,
                            encoder,
                            frame,
                            target_size,
                            &cached.meshes,
                            bounds,
                            scale_factor,
                        );

                        render_pass =
                            ManuallyDrop::new(encoder.begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("iced_wgpu render pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: frame,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Load,
                                                store: wgpu::StoreOp::Store,
                                            },
                                        },
                                    )],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                },
                            ));
                    }

                    if !cached.text.is_empty() {
                        engine.text_pipeline.render_cache(
                            &cached.text,
                            bounds,
                            &mut render_pass,
                        );
                    }

                    #[cfg(any(feature = "svg", feature = "image"))]
                    if !cached.images.is_empty() {
                        engine.image_pipeline.render(
                            image_layer,
                            bounds,
                            &mut render_pass,
                        );

                        image_layer += 1;
                    }
                }
            }
        }

        let _ = ManuallyDrop::into_inner(render_pass);
    }
}

impl core::Renderer for Renderer {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.layers.push_clip(Some(bounds));
    }

    fn end_layer(&mut self, _bounds: Rectangle) {
        self.layers.pop_clip();
    }

    fn start_transformation(&mut self, transformation: Transformation) {
        self.layers.push_transformation(transformation);
    }

    fn end_transformation(&mut self, _transformation: Transformation) {
        self.layers.pop_transformation();
    }

    fn fill_quad(
        &mut self,
        quad: core::renderer::Quad,
        background: impl Into<Background>,
    ) {
        self.layers.draw_quad(quad, background.into());
    }

    fn clear(&mut self) {
        self.layers.clear();
    }
}

impl core::text::Renderer for Renderer {
    type Font = Font;
    type Paragraph = Paragraph;
    type Editor = Editor;

    const ICON_FONT: Font = Font::with_name("Iced-Icons");
    const CHECKMARK_ICON: char = '\u{f00c}';
    const ARROW_DOWN_ICON: char = '\u{e800}';

    fn default_font(&self) -> Self::Font {
        self.default_font
    }

    fn default_size(&self) -> Pixels {
        self.default_text_size
    }

    fn load_font(&mut self, font: Cow<'static, [u8]>) {
        graphics::text::font_system()
            .write()
            .expect("Write font system")
            .load_font(font);

        // TODO: Invalidate buffer cache
    }

    fn fill_paragraph(
        &mut self,
        text: &Self::Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        self.layers
            .draw_paragraph(text, position, color, clip_bounds);
    }

    fn fill_editor(
        &mut self,
        editor: &Self::Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        self.layers
            .draw_editor(editor, position, color, clip_bounds);
    }

    fn fill_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        self.layers.draw_text(text, position, color, clip_bounds);
    }
}

#[cfg(feature = "image")]
impl core::image::Renderer for Renderer {
    type Handle = core::image::Handle;

    fn measure_image(&self, handle: &Self::Handle) -> Size<u32> {
        self.image_cache.lock().measure_image(handle)
    }

    fn draw_image(
        &mut self,
        handle: Self::Handle,
        filter_method: core::image::FilterMethod,
        bounds: Rectangle,
    ) {
        self.layers.draw_image(handle, filter_method, bounds);
    }
}

#[cfg(feature = "svg")]
impl core::svg::Renderer for Renderer {
    fn measure_svg(&self, handle: &core::svg::Handle) -> Size<u32> {
        self.image_cache.lock().measure_svg(handle)
    }

    fn draw_svg(
        &mut self,
        handle: core::svg::Handle,
        color_filter: Option<Color>,
        bounds: Rectangle,
    ) {
        self.layers.draw_svg(handle, color_filter, bounds);
    }
}

impl graphics::mesh::Renderer for Renderer {
    fn draw_mesh(&mut self, mesh: graphics::Mesh) {
        self.layers.draw_mesh(mesh);
    }
}

#[cfg(feature = "geometry")]
impl graphics::geometry::Renderer for Renderer {
    type Geometry = Geometry;
    type Frame = geometry::Frame;

    fn new_frame(&self, size: Size) -> Self::Frame {
        geometry::Frame::new(size)
    }

    fn draw_geometry(&mut self, geometry: Self::Geometry) {
        match geometry {
            Geometry::Live(layers) => {
                for layer in layers {
                    self.layers.draw_layer(layer);
                }
            }
            Geometry::Cached(layers) => {
                for layer in layers.as_ref() {
                    self.layers.draw_cached_layer(layer);
                }
            }
        }
    }
}

impl graphics::compositor::Default for crate::Renderer {
    type Compositor = window::Compositor;
}
