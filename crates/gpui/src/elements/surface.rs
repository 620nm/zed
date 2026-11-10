use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, ObjectFit,
    Pixels, Style, StyleRefinement, Styled, Window,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2_core_foundation::CFRetained;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use refineable::Refineable;
use std::{any::Any, fmt, rc::Rc};

/// A source of a surface's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceSource {
    /// A CoreVideo image buffer
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Surface(CFRetained<CVPixelBuffer>),
    /// Content the window's renderer obtains when it draws the frame.
    Custom(CustomSurface),
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl From<CFRetained<CVPixelBuffer>> for SurfaceSource {
    fn from(value: CFRetained<CVPixelBuffer>) -> Self {
        SurfaceSource::Surface(value)
    }
}

impl From<CustomSurface> for SurfaceSource {
    fn from(value: CustomSurface) -> Self {
        SurfaceSource::Custom(value)
    }
}

/// A renderer-specific content producer, opaque to GPUI's scene: the platform
/// renderer downcasts it to the producer type it supports (for example
/// `gpui_wgpu::WgpuSurfaceProducer`) and skips the surface otherwise.
#[derive(Clone)]
pub struct CustomSurface(Rc<dyn Any>);

impl CustomSurface {
    /// Wrap a renderer's producer.
    pub fn new<T: Any>(producer: T) -> Self {
        Self(Rc::new(producer))
    }

    /// The producer, if it is a `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl PartialEq for CustomSurface {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CustomSurface {}

impl fmt::Debug for CustomSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CustomSurface").finish_non_exhaustive()
    }
}

/// A surface element.
pub struct Surface {
    source: SurfaceSource,
    object_fit: ObjectFit,
    style: StyleRefinement,
}

/// Create a new surface element. A `Custom` source fills the element's bounds.
pub fn surface(source: impl Into<SurfaceSource>) -> Surface {
    Surface {
        source: source.into(),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

impl Surface {
    /// Set the object fit for the image.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        match &self.source {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            SurfaceSource::Surface(surface) => {
                let size = crate::size(
                    CVPixelBufferGetWidth(surface).into(),
                    CVPixelBufferGetHeight(surface).into(),
                );
                let new_bounds = self.object_fit.get_bounds(bounds, size);
                // TODO: Add support for corner_radii
                window.paint_surface(new_bounds, surface.clone());
            }
            SurfaceSource::Custom(custom) => {
                window.paint_custom_surface(bounds, custom.clone());
            }
        }
    }
}

impl Styled for Surface {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
