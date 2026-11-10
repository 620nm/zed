//! Caller-rendered wgpu content in GPUI's scene. A `WgpuSurfaceProducer`,
//! painted with `gpui::surface`, is asked for a texture each time the window's
//! renderer draws a frame; it renders on the renderer's own device, into the
//! frame's encoder, so its commands run before GPUI's pass in one submission.

use gpui::{CustomSurface, DevicePixels, Size};
use std::{cell::RefCell, sync::Arc};

/// What a producer is handed while the renderer records a frame.
pub struct WgpuSurfaceFrame<'a> {
    /// The window renderer's device. It can change (device recovery): compare
    /// with `Arc::ptr_eq` and rebuild resources made on another one.
    pub device: &'a Arc<wgpu::Device>,
    /// The device's queue; buffer writes made here land before this frame.
    pub queue: &'a Arc<wgpu::Queue>,
    /// This frame's encoder, ahead of GPUI's own render pass.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The surface's bounds in device pixels; never zero.
    pub size: Size<DevicePixels>,
}

/// A texture to composite over the surface's bounds: a 2D filterable float
/// color view. Texels of an sRGB `format` land as the same bytes in an image would.
pub struct WgpuSurfaceTexture {
    /// The view GPUI samples.
    pub view: wgpu::TextureView,
    /// The view's format.
    pub format: wgpu::TextureFormat,
}

type Produce = dyn FnMut(WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture>;

/// Renders a surface's content on demand; `None` draws nothing this frame.
pub struct WgpuSurfaceProducer(RefCell<Box<Produce>>);

impl WgpuSurfaceProducer {
    /// A surface source for `gpui::surface` that `produce` fills.
    pub fn new(
        produce: impl FnMut(WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture> + 'static,
    ) -> CustomSurface {
        CustomSurface::new(Self(RefCell::new(Box::new(produce))))
    }

    pub(crate) fn produce(&self, frame: WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture> {
        let mut produce = self.0.try_borrow_mut().ok()?;
        produce(frame)
    }
}
