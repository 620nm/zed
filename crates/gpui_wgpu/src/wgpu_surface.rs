//! Caller-rendered wgpu content in GPUI's scene. A `WgpuSurfaceProducer`,
//! painted with `gpui::surface`, is asked for a texture each time the window's
//! renderer draws a frame; it renders on the renderer's own device, into the
//! frame's encoder, so its commands run before GPUI's pass in one submission.

use gpui::{Bounds, CustomSurface, DevicePixels, Point, ScaledPixels, Size};
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
    /// The surface's bounds in the window, in device pixels. Parts of them may
    /// be clipped away by an ancestor's content mask.
    pub bounds: Bounds<DevicePixels>,
    /// The part of `bounds` that is drawn this frame; never empty.
    pub visible: Bounds<DevicePixels>,
    /// The size of the texture to render `visible` into: `visible.size`, scaled
    /// down to fit the device's maximum texture dimension; never zero.
    pub size: Size<DevicePixels>,
}

/// A texture to stretch over the frame's `visible` bounds: a 2D filterable float
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

/// Where a custom surface is drawn in a frame, in device pixels.
#[derive(Debug, PartialEq)]
pub(crate) struct SurfaceTarget {
    /// The surface's full bounds.
    pub bounds: Bounds<DevicePixels>,
    /// The part of `bounds` inside the content mask; never empty.
    pub visible: Bounds<DevicePixels>,
    /// The size of the texture `visible` is rendered into; never zero.
    pub size: Size<DevicePixels>,
}

/// Where a surface with these bounds and content mask is drawn, fitting its
/// texture to the device's maximum dimension. `None` if nothing is visible.
pub(crate) fn surface_target(
    bounds: Bounds<ScaledPixels>,
    content_mask: Bounds<ScaledPixels>,
    max_texture_dimension: u32,
) -> Option<SurfaceTarget> {
    let bounds = device_bounds(bounds);
    let visible = bounds.intersect(&device_bounds(content_mask));
    if visible.is_empty() {
        return None;
    }
    let max = max_texture_dimension as f64;
    let width = visible.size.width.0 as f64;
    let height = visible.size.height.0 as f64;
    let scale = (max / width).min(max / height).min(1.0);
    let size = Size {
        width: DevicePixels(((width * scale).floor() as i32).max(1)),
        height: DevicePixels(((height * scale).floor() as i32).max(1)),
    };
    Some(SurfaceTarget {
        bounds,
        visible,
        size,
    })
}

/// The device pixels a bounds touches.
fn device_bounds(bounds: Bounds<ScaledPixels>) -> Bounds<DevicePixels> {
    let origin = bounds.origin.map(|coordinate| coordinate.0.floor());
    let bottom_right = bounds.bottom_right().map(|coordinate| coordinate.0.ceil());
    Bounds::from_corners(
        Point::new(DevicePixels(origin.x as i32), DevicePixels(origin.y as i32)),
        Point::new(
            DevicePixels(bottom_right.x as i32),
            DevicePixels(bottom_right.y as i32),
        ),
    )
}
