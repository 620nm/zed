// Modified in the 620nm/zed fork; see NOTICE.
//! Caller-rendered wgpu content in GPUI's scene. A `WgpuSurfaceProducer`,
//! painted with `gpui::surface`, is asked for a texture each time the window's
//! renderer draws a frame. It renders on the renderer's own device, into an
//! encoder submitted just before GPUI's frame, so a validation error in its
//! commands invalidates only its own submission and never GPUI's.

use gpui::{Bounds, CustomSurface, DevicePixels, Point, ScaledPixels, Size};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// Consecutive frames with a validation error after which a producer is no
/// longer called.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// What a producer is handed while the renderer records a frame.
pub struct WgpuSurfaceFrame<'a> {
    /// The window renderer's device. It can change (device recovery): compare
    /// with `Arc::ptr_eq` and rebuild resources made on another one.
    pub device: &'a Arc<wgpu::Device>,
    /// The device's queue; buffer writes made here land before this frame.
    pub queue: &'a Arc<wgpu::Queue>,
    /// An encoder for the producer's commands, submitted before GPUI's frame.
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
type ErrorScopeResult = Pin<Box<dyn Future<Output = Option<wgpu::Error>>>>;

/// Renders a surface's content on demand; `None` draws nothing this frame.
///
/// Its work runs inside a validation error scope. A frame whose work raises an
/// error is logged and not drawn, and after a few such frames in a row the
/// producer is no longer called. Where the scope's result arrives later (the
/// browser's WebGPU), the frames before it draws whatever the producer returned.
pub struct WgpuSurfaceProducer(RefCell<ProducerState>);

struct ProducerState {
    produce: Box<Produce>,
    pending_errors: Vec<ErrorScopeResult>,
    consecutive_failures: u32,
}

impl WgpuSurfaceProducer {
    /// A surface source for `gpui::surface` that `produce` fills.
    pub fn new(
        produce: impl FnMut(WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture> + 'static,
    ) -> CustomSurface {
        CustomSurface::new(Self(RefCell::new(ProducerState {
            produce: Box::new(produce),
            pending_errors: Vec::new(),
            consecutive_failures: 0,
        })))
    }

    /// Runs the producer and submits its commands, unless it has failed too often.
    pub(crate) fn produce(
        &self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        bounds: Bounds<DevicePixels>,
        visible: Bounds<DevicePixels>,
        size: Size<DevicePixels>,
    ) -> Option<WgpuSurfaceTexture> {
        let mut state = self.0.try_borrow_mut().ok()?;
        state.collect_errors();
        if state.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return None;
        }

        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("custom_surface_encoder"),
        });
        let texture = (state.produce)(WgpuSurfaceFrame {
            device,
            queue,
            encoder: &mut encoder,
            bounds,
            visible,
            size,
        });
        queue.submit(std::iter::once(encoder.finish()));
        state.pending_errors.push(Box::pin(error_scope.pop()));

        let failures = state.consecutive_failures;
        state.collect_errors();
        if state.consecutive_failures > failures {
            return None;
        }
        texture
    }
}

impl ProducerState {
    /// Counts the error scope results that have arrived, in submission order.
    fn collect_errors(&mut self) {
        let mut context = Context::from_waker(Waker::noop());
        while let Some(result) = self.pending_errors.first_mut() {
            let Poll::Ready(error) = result.as_mut().poll(&mut context) else {
                break;
            };
            drop(self.pending_errors.remove(0));
            let Some(error) = error else {
                self.consecutive_failures = 0;
                continue;
            };
            self.consecutive_failures += 1;
            log::error!(
                "custom surface producer failed (failure {} of {MAX_CONSECUTIVE_FAILURES}): {error}",
                self.consecutive_failures
            );
            if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                log::error!("custom surface producer keeps failing; it is no longer drawn");
                self.pending_errors.clear();
            }
        }
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
