use crate::node::{Node, Nodes};
use crate::plugin::Plugin;
use lilv_sys as lib;
use std::fmt::Debug;
use std::ptr::NonNull;

#[derive(Clone)]
pub struct Port {
    pub(crate) inner: NonNull<lib::LilvPort>,
    pub(crate) plugin: Plugin,
}

impl Port {
    /// Get the node for the port.
    ///
    /// # Panics
    /// Panics if the node could not be obtained.
    #[must_use]
    pub fn node(&self) -> Node {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();

        {
            let ptr = NonNull::new(unsafe { lib::lilv_port_get_node(plugin, port) as _ }).unwrap();
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        }
    }

    /// Get the value associated with the port in a plugin's data files.
    ///
    /// `predicate` must be either a URI or a `QName`.
    /// Returns the ?object of all triples found of the form:
    ///     `<plugin-uri> predicate ?object`
    #[must_use]
    pub fn value(&self, predicate: &Node) -> Nodes {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let predicate = predicate.inner.as_ptr();
        let inner = unsafe { lib::lilv_port_get_value(plugin, port, predicate) };
        let world = self.plugin.life.clone();
        Nodes { inner, life: world }
    }

    /// Get a single property value of a port.
    ///
    /// This is equivalent to getting the first iterable value of
    /// `self.value(predicate)`.
    #[must_use]
    pub fn get(&self, predicate: &Node) -> Option<Node> {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let predicate = predicate.inner.as_ptr();

        Some({
            let ptr = NonNull::new(unsafe { lib::lilv_port_get(plugin, port, predicate) })?;
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        })
    }

    /// Return the LV2 port properties of a port.
    #[must_use]
    pub fn properties(&self) -> Nodes {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let inner = unsafe { lib::lilv_port_get_properties(plugin, port) };
        let world = self.plugin.life.clone();
        Nodes { inner, life: world }
    }

    /// Returns true if the port has the given property.
    #[must_use]
    pub fn has_property(&self, property_uri: &Node) -> bool {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let property_uri = property_uri.inner.as_ptr();

        unsafe { lib::lilv_port_has_property(plugin, port, property_uri) }
    }

    /// Returns `true` if the port supports a certain event type.
    ///
    /// More precisely, this returns `true` if and only iff the port has an
    /// `atom:supports` or an `ev:supportsEvent` property with `event_type` as
    /// the value.
    #[must_use]
    pub fn supports_event(&self, event_type: &Node) -> bool {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let event_type = event_type.inner.as_ptr();

        unsafe { lib::lilv_port_supports_event(plugin, port, event_type) }
    }

    /// Returns the index of the port within the plugin.
    #[must_use]
    pub fn index(&self) -> usize {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();

        unsafe { lib::lilv_port_get_index(plugin, port) as _ }
    }

    /// Get the symbol of a port.
    ///
    /// Symbol is a short string.
    #[must_use]
    pub fn symbol(&self) -> Option<Node> {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();

        {
            let ptr = NonNull::new(unsafe { lib::lilv_port_get_symbol(plugin, port) as _ })?;
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        }
        .into()
    }

    /// Get the name of a port.
    ///
    /// The is guaranteed to return the untraslated name (the doap:name in the
    /// data file without a language tag).
    #[must_use]
    pub fn name(&self) -> Option<Node> {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();

        Some({
            let ptr = NonNull::new(unsafe { lib::lilv_port_get_name(plugin, port) })?;
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        })
    }

    /// Get all the classes of the port.
    ///
    /// This can be used to determine if a port is an input, output, audio,
    /// control, midi, etc... although it's simpler to use `Port::is_a`.
    #[must_use]
    pub fn classes(&self) -> Nodes {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let inner = unsafe { lib::lilv_port_get_classes(plugin, port) };
        let world = self.plugin.life.clone();
        Nodes { inner, life: world }
    }

    /// Returns `true` if the port is of the given type.
    #[must_use]
    pub fn is_a(&self, port_class: &Node) -> bool {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();
        let port_class = port_class.inner.as_ptr();

        unsafe { lib::lilv_port_is_a(plugin, port, port_class) }
    }

    /// The the range (default, minimum, maximum) values of the port.
    ///
    /// # Panics
    /// Panics if the range could not be obtained.
    #[must_use]
    pub fn range(&self) -> Range {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr();
        let port = self.inner.as_ptr();

        let mut default_ptr: *mut lib::LilvNodeImpl = std::ptr::null_mut();
        let mut minimum_ptr: *mut lib::LilvNodeImpl = std::ptr::null_mut();
        let mut maximum_ptr: *mut lib::LilvNodeImpl = std::ptr::null_mut();

        unsafe {
            lib::lilv_port_get_range(
                plugin,
                port,
                &mut default_ptr,
                &mut minimum_ptr,
                &mut maximum_ptr,
            );
        };
        let ptr_to_node = |ptr: *mut lib::LilvNodeImpl| -> Option<Node> {
            let ptr = NonNull::new(ptr.cast())?;
            let world = self.plugin.life.clone();
            Some(Node {
                inner: ptr,
                borrowed: false,
                life: world,
            })
        };
        Range {
            default: ptr_to_node(default_ptr),
            minimum: ptr_to_node(minimum_ptr),
            maximum: ptr_to_node(maximum_ptr),
        }
    }

    /// Get the scale points (enumeration values) of a port.
    ///
    /// This returns a collection of "interesting" named values of a port. These
    /// are appropriate entries for a UI selector.
    #[must_use]
    pub fn scale_points(&self) -> ScalePoints {
        let _life = self.plugin.life.inner.lock();
        let plugin = self.plugin.inner.as_ptr() as *const _;
        let port = self.inner.as_ptr() as *const _;

        ScalePoints {
            inner: unsafe { lib::lilv_port_get_scale_points(plugin, port) },
            port: self.clone(),
            refs: std::sync::Arc::new(1.into()),
        }
    }
}

impl Debug for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Port")
            .field("name", &self.name())
            .field("symbol", &self.symbol())
            .field("classes", &self.classes())
            .field("range", &self.range())
            .field("properties", &self.properties())
            .finish()
    }
}

unsafe impl Sync for ScalePoint {}

#[derive(Clone)]
pub struct ScalePoint {
    pub(crate) inner: NonNull<lib::LilvScalePoint>,
    pub(crate) port: Port,

    // The underlying ScalePoints must be kept alive for the lifetime of the
    // ScalePoint object.
    _collection: ScalePoints,
}

impl ScalePoint {
    /// Get the label of the scale point (enumeration value).
    ///
    /// # Panics
    /// Panics if the node for the value could not be obtained.
    #[must_use]
    pub fn label(&self) -> Node {
        let _life = self.port.plugin.life.inner.lock();
        let inner = self.inner.as_ptr();

        {
            let ptr = NonNull::new(unsafe { lib::lilv_scale_point_get_label(inner) as _ }).unwrap();
            let world = self.port.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        }
    }

    /// Get the value of the scale point (enumeration value).
    ///
    /// # Panics
    /// Panics if the node for the value could not be obtained.
    #[must_use]
    pub fn value(&self) -> Node {
        let _life = self.port.plugin.life.inner.lock();
        let inner = self.inner.as_ptr();

        {
            let ptr = NonNull::new(unsafe { lib::lilv_scale_point_get_value(inner) as _ }).unwrap();
            let world = self.port.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        }
    }
}

impl Debug for ScalePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScalePoint")
            .field("label", &self.label())
            .field("value", &self.value())
            .finish()
    }
}

pub struct ScalePoints {
    pub(crate) inner: *const lib::LilvScalePoints,
    pub(crate) port: Port,
    pub(crate) refs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ScalePoints {
    /// The number of scale points within the collection.
    #[must_use]
    pub fn count(&self) -> usize {
        let _life = self.port.plugin.life.inner.lock();
        let size: u32 = unsafe { lib::lilv_scale_points_size(self.inner) };
        size as usize
    }

    /// An iterator over the scale points in the collection.
    #[must_use]
    pub fn iter(&self) -> ScalePointsIter {
        let _life = self.port.plugin.life.inner.lock();
        ScalePointsIter {
            inner: self.clone(),
            iter: unsafe { lib::lilv_scale_points_begin(self.inner) },
        }
    }
}

impl Drop for ScalePoints {
    fn drop(&mut self) {
        let refs = self.refs.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if refs == 0 {
            unsafe {
                lib::lilv_scale_points_free(self.inner as *mut _);
            }
        }
    }
}

impl Clone for ScalePoints {
    fn clone(&self) -> Self {
        self.refs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self {
            inner: self.inner,
            port: self.port.clone(),
            refs: self.refs.clone(),
        }
    }
}

impl Debug for ScalePoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pts = self.iter().collect::<Vec<_>>();
        f.debug_struct("ScalePoints")
            .field("scale_points", &pts)
            .finish()
    }
}

impl IntoIterator for ScalePoints {
    type Item = ScalePoint;

    type IntoIter = ScalePointsIter;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over scale points.
#[derive(Clone)]
pub struct ScalePointsIter {
    inner: ScalePoints,
    iter: *mut lib::LilvIter,
}

impl Iterator for ScalePointsIter {
    type Item = ScalePoint;

    fn next(&mut self) -> Option<ScalePoint> {
        let _life = self.inner.port.plugin.life.inner.lock();
        let next_ptr =
            unsafe { lib::lilv_scale_points_get(self.inner.inner, self.iter.cast()) } as *mut _;
        let next = Some(ScalePoint {
            inner: NonNull::new(next_ptr)?,
            port: self.inner.port.clone(),
            _collection: self.inner.clone(),
        });
        self.iter = unsafe { lib::lilv_scale_points_next(self.inner.inner, self.iter) };
        next
    }
}

/// Describes the ranges of the port if possible.
#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    /// The default value of the port.
    pub default: Option<Node>,
    /// The minimum value of the port.
    pub minimum: Option<Node>,
    /// The maximum value of the port.
    pub maximum: Option<Node>,
}

/// Describes the range of the ports of a plugin.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FloatRanges {
    /// The default value of the port.
    pub default: f32,
    /// The minimum value of the port.
    pub min: f32,
    /// The maximum value of the port.
    pub max: f32,
}
