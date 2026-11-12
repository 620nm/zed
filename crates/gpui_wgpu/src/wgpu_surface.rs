//! Caller-rendered wgpu content in GPUI's scene. A [`WgpuSurfaceProducer`],
//! painted with `gpui::surface`, is asked for a texture each time the window's
//! renderer draws a frame, and GPUI draws that texture in paint order.

use gpui::{Bounds, CustomSurface, DevicePixels, Point, ScaledPixels, Size};
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
};

/// Consecutive failed frames after which a producer is no longer called on
/// the same device.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// What a producer is handed while the renderer records a frame.
pub struct WgpuSurfaceFrame<'a> {
    /// The window renderer's device. It is replaced when the renderer recovers
    /// from a lost device: compare it with `Arc::ptr_eq` and rebuild resources
    /// made on another one.
    pub device: &'a Arc<wgpu::Device>,
    /// The device's queue. Writes made here land before this frame is drawn.
    pub queue: &'a Arc<wgpu::Queue>,
    /// An encoder for the producer's commands, submitted before GPUI's frame.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The surface's bounds in the window, in device pixels. Parts of them may
    /// be clipped away by an ancestor's content mask or the window's edges.
    pub bounds: Bounds<DevicePixels>,
    /// The part of `bounds` that is drawn this frame; never empty.
    pub visible: Bounds<DevicePixels>,
    /// The size of `visible`: a texture of this size maps one texel to each
    /// device pixel. Never zero, and never above the device's maximum texture
    /// dimension. It changes with the window's size and scale factor.
    pub size: Size<DevicePixels>,
}

/// A texture to stretch over the frame's `visible` bounds.
///
/// The view must be a single-sampled 2D view with `TEXTURE_BINDING` usage, in
/// a filterable float format, created on the frame's device, and its texture
/// must not be destroyed before the frame is drawn. Its texels are
/// premultiplied by alpha, as GPU rendering with premultiplied blending
/// produces them. With an sRGB `format`, they hold linear color premultiplied
/// in linear space, which the hardware decodes on sampling; with any other
/// format, they hold sRGB-encoded color, the encoding GPUI blends in,
/// premultiplied in that encoding.
pub struct WgpuSurfaceTexture {
    /// The view GPUI samples.
    pub view: wgpu::TextureView,
    /// The view's format.
    pub format: wgpu::TextureFormat,
}

type Produce = dyn FnMut(WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture>;
type ErrorScopeResult = Pin<Box<dyn Future<Output = Option<wgpu::Error>>>>;

/// Renders a surface's content on demand; returning `None` draws nothing this
/// frame.
///
/// Each call runs inside error scopes that capture validation, out-of-memory
/// and internal errors, together with GPUI's creation of the bind group for
/// the returned view, and its encoder is submitted on its own just before
/// GPUI's frame. A failing producer therefore loses only its own surface: a
/// frame whose scopes report an error, or whose view GPUI cannot sample, is
/// logged and not drawn. After a few such frames in a row the producer is no
/// longer called, until the renderer's device is replaced (device-loss
/// recovery); to try again on the same device, paint a new producer.
///
/// Where error scopes resolve asynchronously (WebGPU in a browser), a frame is
/// drawn before its result arrives, so an invalid view that passes GPUI's
/// checks of its texture can still fail that frame of GPUI's.
pub struct WgpuSurfaceProducer(RefCell<ProducerState>);

struct ProducerState {
    produce: Box<Produce>,
    device: Weak<wgpu::Device>,
    pending_errors: VecDeque<ErrorScopeResult>,
    consecutive_failures: u32,
}

impl WgpuSurfaceProducer {
    /// A surface source for `gpui::surface` that `produce` fills.
    pub fn new(
        produce: impl FnMut(WgpuSurfaceFrame<'_>) -> Option<WgpuSurfaceTexture> + 'static,
    ) -> CustomSurface {
        CustomSurface::new(Self(RefCell::new(ProducerState {
            produce: Box::new(produce),
            device: Weak::new(),
            pending_errors: VecDeque::new(),
            consecutive_failures: 0,
        })))
    }

    /// Runs the producer and submits its commands, unless it has failed too
    /// often on this device, and binds the texture it returns with `bind`.
    pub(crate) fn produce<T>(
        &self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        target: &SurfaceTarget,
        bind: impl FnOnce(&WgpuSurfaceTexture) -> T,
    ) -> Option<T> {
        let mut state = self.0.try_borrow_mut().ok()?;
        if !Weak::ptr_eq(&state.device, &Arc::downgrade(device)) {
            state.device = Arc::downgrade(device);
            state.pending_errors.clear();
            state.consecutive_failures = 0;
        }
        state.collect_errors();
        if state.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return None;
        }

        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("custom_surface_encoder"),
        });
        let texture = (state.produce)(WgpuSurfaceFrame {
            device,
            queue,
            encoder: &mut encoder,
            bounds: target.bounds,
            visible: target.visible,
            size: target.size,
        });
        let mut unsampleable = None;
        let binding = match &texture {
            Some(texture) => match unsampleable_reason(texture, device) {
                Some(reason) => {
                    unsampleable = Some(wgpu::Error::Validation {
                        source: reason.into(),
                        description: reason.to_string(),
                    });
                    None
                }
                None => Some(bind(texture)),
            },
            None => None,
        };
        // Submitted alone: a submission with any invalid command buffer is
        // rejected whole, which would take other surfaces' work down with it.
        queue.submit(std::iter::once(encoder.finish()));
        let results = scopes
            .into_iter()
            .rev()
            .map(|scope| scope.pop())
            .collect::<Vec<_>>();
        state.pending_errors.push_back(Box::pin(async move {
            if unsampleable.is_some() {
                return unsampleable;
            }
            for result in results {
                if let Some(error) = result.await {
                    return Some(error);
                }
            }
            None
        }));

        let failures = state.consecutive_failures;
        state.collect_errors();
        if state.consecutive_failures > failures {
            return None;
        }
        binding
    }
}

/// Why GPUI cannot sample `texture` through its surfaces bind group layout,
/// checked up front so that an unusable view never reaches a bind group where
/// error scopes resolve asynchronously.
fn unsampleable_reason(
    texture: &WgpuSurfaceTexture,
    device: &wgpu::Device,
) -> Option<&'static str> {
    let source = texture.view.texture();
    if !source
        .usage()
        .contains(wgpu::TextureUsages::TEXTURE_BINDING)
    {
        Some("surface texture lacks TEXTURE_BINDING usage")
    } else if source.sample_count() != 1 {
        Some("surface texture is multisampled")
    } else if source.dimension() != wgpu::TextureDimension::D2 {
        Some("surface texture is not two-dimensional")
    } else if texture.format.sample_type(None, Some(device.features()))
        != Some(wgpu::TextureSampleType::Float { filterable: true })
    {
        Some("surface view format is not filterable float")
    } else {
        None
    }
}

impl ProducerState {
    /// Counts the error scope results that have arrived, in submission order.
    fn collect_errors(&mut self) {
        let mut context = Context::from_waker(Waker::noop());
        while let Some(result) = self.pending_errors.front_mut() {
            let Poll::Ready(error) = result.as_mut().poll(&mut context) else {
                break;
            };
            self.pending_errors.pop_front();
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
                log::error!(
                    "custom surface producer keeps failing; it is no longer drawn on this device"
                );
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
    /// The part of `bounds` inside the content mask and the viewport; never empty.
    pub visible: Bounds<DevicePixels>,
    /// The size of `visible`.
    pub size: Size<DevicePixels>,
}

/// Where a surface with these bounds and content mask is drawn in a viewport
/// of `viewport_size`, or `None` if none of it is visible.
pub(crate) fn surface_target(
    bounds: Bounds<ScaledPixels>,
    content_mask: Bounds<ScaledPixels>,
    viewport_size: Size<DevicePixels>,
) -> Option<SurfaceTarget> {
    let bounds = device_bounds(bounds);
    let viewport = Bounds {
        origin: Point::default(),
        size: viewport_size,
    };
    let visible = bounds
        .intersect(&device_bounds(content_mask))
        .intersect(&viewport);
    if visible.is_empty() {
        return None;
    }
    Some(SurfaceTarget {
        bounds,
        visible,
        size: visible.size,
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
