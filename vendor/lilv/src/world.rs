use crate::node::{Node, Nodes};
use crate::plugin::Class;
use crate::plugin::Plugins;
use lilv_sys as lib;
use parking_lot::Mutex;
use std::ptr::NonNull;
use std::sync::Arc;

unsafe impl Send for Life {}
unsafe impl Sync for Life {}

/// The world represents all Lilv state. It is used to discover/load/cache LV2
/// data (plugins, UIs, and extensions).
pub struct World {
    pub(crate) life: Arc<Life>,
}

#[doc(hidden)]
#[allow(clippy::non_send_fields_in_send_ty)]
pub struct Life {
    pub(crate) inner: Mutex<NonNull<lib::LilvWorldImpl>>,
}

impl World {
    /// Initializes a new, empty world.
    ///
    /// # Panics
    /// Panics if the world could not be created.
    #[must_use]
    pub fn new() -> Self {
        Self {
            life: Arc::new(Life {
                inner: Mutex::new(NonNull::new(unsafe { lib::lilv_world_new() }).unwrap()),
            }),
        }
    }

    /// Loads a new world with all the installed LV2 bundles on the system.
    ///
    /// # Example
    /// ```
    /// let world = lilv::World::new();
    /// world.load_all();
    /// ```
    #[must_use]
    pub fn with_load_all() -> World {
        let world = World::new();
        world.load_all();
        world
    }
}

impl World {
    /// Get the parent of all other plugin classes, lv2:Plugin.
    #[must_use]
    pub fn plugin_class(&self) -> Option<Class> {
        let world = self.life.inner.lock();
        Some({
            let ptr =
                NonNull::new(unsafe { lib::lilv_world_get_plugin_class(world.as_ptr()) as _ })?;
            let world = self.life.clone();
            Class {
                inner: ptr,
                life: world,
            }
        })
    }

    /// An iterable over all the plugins in the world.
    #[must_use]
    pub fn plugins(&self) -> Plugins {
        let world = self.life.inner.lock();
        let ptr = unsafe { lib::lilv_world_get_all_plugins(world.as_ptr()) };

        Plugins {
            life: self.life.clone(),
            ptr,
        }
    }
}

impl World {
    /// Sets an option for the world.
    /// # Panics
    /// Panics if uri could not be converted to a `CString`.
    pub fn set_option(&self, uri: &str, value: &Node) {
        let world = self.life.inner.lock();
        let uri = std::ffi::CString::new(uri).unwrap();
        let value = value.inner.as_ptr();

        unsafe { lib::lilv_world_set_option(world.as_ptr(), uri.as_ptr().cast(), value) }
    }
}

impl World {
    /// Creates a new URI value.
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_uri(&self, uri: &str) -> Node {
        let world = self.life.inner.lock();
        let uri = std::ffi::CString::new(uri).unwrap();

        {
            let ptr =
                NonNull::new(unsafe { lib::lilv_new_uri(world.as_ptr(), uri.as_ptr().cast()) })
                    .unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }

    /// Creates a new file URI value.
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_file_uri(&self, host: Option<&str>, path: &str) -> Node {
        let world = self.life.inner.lock();
        let host = host
            .iter()
            .find_map(|h| std::ffi::CString::new(h.as_bytes()).ok());
        let path = std::ffi::CString::new(path.as_bytes()).unwrap();

        let host_ptr = host.map_or(std::ptr::null(), |h| h.as_ptr());
        let path_ptr = path.as_ptr();

        {
            let ptr =
                NonNull::new(unsafe { lib::lilv_new_file_uri(world.as_ptr(), host_ptr, path_ptr) })
                    .unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }

    /// Creates a new string value (with no language).
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_string(&self, string: &str) -> Node {
        let world = self.life.inner.lock();
        let string = std::ffi::CString::new(string).unwrap();

        {
            let ptr =
                NonNull::new(unsafe { lib::lilv_new_string(world.as_ptr(), string.as_ptr()) })
                    .unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }

    /// Creates a new integer value.
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_int(&self, value: i32) -> Node {
        let world = self.life.inner.lock();
        {
            let ptr = NonNull::new(unsafe { lib::lilv_new_int(world.as_ptr(), value) }).unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }

    /// Creates a new floating point value.
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_float(&self, value: f32) -> Node {
        let world = self.life.inner.lock();
        {
            let ptr = NonNull::new(unsafe { lib::lilv_new_float(world.as_ptr(), value) }).unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }

    /// Creates a new boolean value.
    ///
    /// # Panics
    /// Panics on failure.
    #[must_use]
    pub fn new_bool(&self, value: bool) -> Node {
        let world = self.life.inner.lock();
        {
            let ptr = NonNull::new(unsafe { lib::lilv_new_bool(world.as_ptr(), value) }).unwrap();
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        }
    }
}

impl World {
    /// Loads all installed LV2 bundles on the system.
    ///
    /// # Example
    /// ```
    /// let world = lilv::World::new();
    /// world.load_all();
    /// ```
    pub fn load_all(&self) {
        let world = self.life.inner.lock();
        unsafe { lib::lilv_world_load_all(world.as_ptr()) }
    }

    /// Loads a specific bundle. `bundle_uri` must be a fully qualified URI to the bundle directory,
    /// with the trailing slash, eg `file:///usr/lib/lv2/foo.lv2/`.
    pub fn load_bundle(&self, bundle_uri: &Node) {
        let world = self.life.inner.lock();
        let bundle_uri = bundle_uri.inner.as_ptr();

        unsafe { lib::lilv_world_load_bundle(world.as_ptr(), bundle_uri) }
    }

    /// Loads all specifications from currently loaded bundles.
    ///
    /// This is for hosts that explicitly load specific bundles, its use is not
    /// necessary when using [`load_all`](#method.load_all). This function parses the specifications
    /// and adds them to the model.
    pub fn load_specifications(&self) {
        let world = self.life.inner.lock();
        unsafe { lib::lilv_world_load_specifications(world.as_ptr()) }
    }

    /// Load all plugin classes from currently loaded specifications.
    ///
    /// Must be called after [`load_specifications`](#method.load_specifications). This is for hosts
    /// that explicitly load specific bundles; its use is not necessary when using
    /// [`load_all`](#method.load_all).
    pub fn load_plugin_classes(&self) {
        let world = self.life.inner.lock();
        unsafe { lib::lilv_world_load_plugin_classes(world.as_ptr()) }
    }

    /// Unload a specific bundle.
    ///
    /// This unloads statements loaded by `load_bundle`. Note this is not necessarily all
    /// information loaded from the bundle. If any  resources have been separately loaded with
    /// [`load_resource`](#method.load_resource), they must be separately unloaded with
    /// [`unload_resource`](#method.unload_resource).
    ///
    /// # Safety
    /// Unloading bundles that are in use by the host will cause undefined
    /// behaviour.
    #[must_use]
    pub unsafe fn unload_bundle(&self, bundle_uri: &Node) -> bool {
        let world = self.life.inner.lock();
        let bundle_uri = bundle_uri.inner.as_ptr();

        lib::lilv_world_unload_bundle(world.as_ptr(), bundle_uri) == 0
    }

    /// Load all the data associated with the given resource.
    ///
    /// # Return
    /// The number of files parsed.
    #[allow(clippy::cast_sign_loss)]
    #[must_use]
    pub fn load_resource(&self, resource: &Node) -> Option<usize> {
        let world = self.life.inner.lock();
        let resource = resource.inner.as_ptr();

        match unsafe { lib::lilv_world_load_resource(world.as_ptr(), resource) } {
            -1 => None,
            n => Some(n as usize),
        }
    }

    /// Unload all the data associated with the given resource.
    ///
    /// # Safety
    /// Unloading resources that are in use by the host will cause undefined
    /// behaviour.
    #[must_use]
    pub unsafe fn unload_resource(&self, resource: &Node) -> bool {
        let world = self.life.inner.lock();
        let resource = resource.inner.as_ptr();

        lib::lilv_world_unload_resource(world.as_ptr(), resource) == 0
    }
}

impl World {
    /// Find nodes matching a triple pattern. Either subject or object may be `None`, but not both.
    #[must_use]
    pub fn find_nodes(
        &self,
        subject: Option<&Node>,
        predicate: &Node,
        object: Option<&Node>,
    ) -> Nodes {
        let world = self.life.inner.lock();
        let subject = subject.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let predicate = predicate.inner.as_ptr();
        let object = object.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let inner =
            unsafe { lib::lilv_world_find_nodes(world.as_ptr(), subject, predicate, object) };
        let world = self.life.clone();
        Nodes { inner, life: world }
    }

    /// Find a single node that matches a pattern. Exactly one of `subject`, `predicate`, or
    /// `object` must be `None`.
    #[must_use]
    pub fn get(
        &self,
        subject: Option<&Node>,
        predicate: Option<&Node>,
        object: Option<&Node>,
    ) -> Option<Node> {
        let world = self.life.inner.lock();
        let subject = subject.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let predicate = predicate.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let object = object.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);

        Some({
            let ptr = NonNull::new(unsafe {
                lib::lilv_world_get(world.as_ptr(), subject, predicate, object)
            })?;
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        })
    }

    /// Returns true iff a statement matching a certain pattern exists.
    #[must_use]
    pub fn ask(
        &self,
        subject: Option<&Node>,
        predicate: Option<&Node>,
        object: Option<&Node>,
    ) -> bool {
        let world = self.life.inner.lock();
        let subject = subject.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let predicate = predicate.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);
        let object = object.map_or(std::ptr::null(), |n| n.inner.as_ptr() as _);

        unsafe { lib::lilv_world_ask(world.as_ptr(), subject, predicate, object) }
    }

    /// Get an LV2 symbol for some subject.
    ///
    /// This will return the lv2:symbol property of the subject if it is given explicitly. Otherwise
    /// it will attempt to derive a symbol from the URI.
    #[must_use]
    pub fn symbol(&self, subject: &Node) -> Option<Node> {
        let world = self.life.inner.lock();
        let subject = subject.inner.as_ptr();

        Some({
            let ptr = NonNull::new(unsafe { lib::lilv_world_get_symbol(world.as_ptr(), subject) })?;
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: false,
                life: world,
            }
        })
    }
}

impl World {
    /// Get the underlying pointer to the `World`.
    pub fn as_ptr(&self) -> *mut lib::LilvWorldImpl {
        let inner = self.life.inner.lock();
        inner.as_ptr()
    }
}

impl Default for World {
    /// Return a new empty world.
    fn default() -> World {
        World::new()
    }
}

impl Drop for Life {
    fn drop(&mut self) {
        unsafe {
            let world = self.inner.lock();
            lib::lilv_world_free(world.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_all() {
        let w = World::new();
        w.load_all();
    }

    #[test]
    fn test_new_node() {
        let w = World::new();
        assert!(w.new_bool(true).is_bool());
        assert!(w.new_float(0.1).is_float());
        assert!(w.new_int(1).is_int());
        assert!(w.new_file_uri(None, "/some/path").is_uri());
        assert!(w.new_file_uri(Some("me"), "/some/path").is_uri());
        assert!(w.new_string("string").is_string());
    }
}
