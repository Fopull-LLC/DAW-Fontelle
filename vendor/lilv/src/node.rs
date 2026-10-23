use crate::world::Life;
use lilv_sys as lib;
use std::borrow::Borrow;
use std::ffi::CStr;
use std::fmt::Debug;
use std::ptr::NonNull;
use std::sync::Arc;

extern "C" {
    // Needed for Node::get_path as it actually returns
    // things allocated in Serd.
    fn serd_free(ptr: *mut std::os::raw::c_void);
}

unsafe impl Send for Node {}
unsafe impl Sync for Node {}

pub struct Node {
    pub(crate) inner: NonNull<lib::LilvNodeImpl>,
    pub(crate) borrowed: bool,
    pub(crate) life: Arc<Life>,
}

impl Node {
    /// Returns this value as a Turtle/SPARQL token.
    #[must_use]
    pub fn turtle_token(&self) -> String {
        let _life = self.life.inner.lock();
        let node = self.inner.as_ptr();

        unsafe {
            let raw = lib::lilv_node_get_turtle_token(node);
            let formatted = CStr::from_ptr(raw).to_string_lossy().into_owned();
            lib::lilv_free(raw.cast());
            formatted
        }
    }

    /// Returns whether the value is a URI (resource).
    #[must_use]
    pub fn is_uri(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_uri(self.inner.as_ptr()) }
    }

    /// Returns this value as a URI string or `None` if it is not a uri.
    #[must_use]
    pub fn as_uri(&self) -> Option<&str> {
        if self.is_uri() {
            let _life = self.life.inner.lock();
            Some(unsafe {
                CStr::from_ptr(lib::lilv_node_as_uri(self.inner.as_ptr()))
                    .to_str()
                    .ok()?
            })
        } else {
            None
        }
    }

    /// Returns whether the value is a blank node (resource with no URI).
    #[must_use]
    pub fn is_blank(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_blank(self.inner.as_ptr()) }
    }

    /// Returns this value as a blank node identifier.
    #[must_use]
    pub fn as_blank(&self) -> Option<&str> {
        if self.is_blank() {
            let _life = self.life.inner.lock();
            Some(unsafe {
                CStr::from_ptr(lib::lilv_node_as_blank(self.inner.as_ptr()))
                    .to_str()
                    .ok()?
            })
        } else {
            None
        }
    }

    /// Returns `true` if the value is a literal (i.e. not a URI).
    /// Returns `true` if the value is a string or numeric value.
    #[must_use]
    pub fn is_literal(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_literal(self.inner.as_ptr()) }
    }

    /// Returns whether this value is a string literal.
    #[must_use]
    pub fn is_string(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_string(self.inner.as_ptr()) }
    }

    /// Return the value as a string or `None` if it is not a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let _life = self.life.inner.lock();
        Some(unsafe {
            CStr::from_ptr(lib::lilv_node_as_string(self.inner.as_ptr()))
                .to_str()
                .ok()?
        })
    }

    /// Return the path of a file URI node or `None` if it is not a file URI.
    /// The returned values are `(hostname, path)`
    #[must_use]
    pub fn path(&self) -> Option<(String, String)> {
        let _life = self.life.inner.lock();
        let node = self.inner.as_ptr();
        let mut raw_hostname = std::ptr::null_mut();
        let raw_path = NonNull::new(unsafe { lib::lilv_node_get_path(node, &mut raw_hostname) })?;

        let path = unsafe { CStr::from_ptr(raw_path.as_ptr()) }
            .to_string_lossy()
            .into_owned();

        let hostname = if !raw_hostname.is_null() {
            let hostname = unsafe { CStr::from_ptr(raw_hostname) }
                .to_string_lossy()
                .into_owned();
            unsafe { serd_free(raw_hostname.cast()) };
            hostname
        } else {
            "".to_string()
        };

        unsafe { serd_free(raw_path.as_ptr().cast()) };

        Some((hostname, path))
    }

    /// Returns `true` if the value is a decimal literal.
    #[must_use]
    pub fn is_float(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_float(self.inner.as_ptr()) }
    }

    /// Returns the value as an `f32` if it is an int or float. `None` is
    /// returned otherwise.
    #[must_use]
    pub fn as_float(&self) -> Option<f32> {
        if self.is_float() {
            let _life = self.life.inner.lock();
            Some(unsafe { lib::lilv_node_as_float(self.inner.as_ptr()) })
        } else {
            None
        }
    }

    /// Returns `true` if the value is an integer literal.
    #[must_use]
    pub fn is_int(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_int(self.inner.as_ptr()) }
    }

    /// Returns the value as an `i32` if it is an int (not float) or `None`
    /// otherwise.
    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        if self.is_int() {
            let _life = self.life.inner.lock();
            Some(unsafe { lib::lilv_node_as_int(self.inner.as_ptr()) })
        } else {
            None
        }
    }

    /// Returns `true` if the value is a boolean literal.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_is_bool(self.inner.as_ptr()) }
    }

    /// Returns the value as a `bool` if it is a boolean or `None` otherwise.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        if self.is_bool() {
            let _life = self.life.inner.lock();
            Some(unsafe { lib::lilv_node_as_bool(self.inner.as_ptr()) })
        } else {
            None
        }
    }

    /// Return the underlying node pointer.
    pub fn as_ptr(&self) -> *mut lib::LilvNodeImpl {
        self.inner.as_ptr()
    }
}

impl Clone for Node {
    fn clone(&self) -> Self {
        let _life = self.life.inner.lock();
        Self {
            inner: NonNull::new(unsafe { lib::lilv_node_duplicate(self.inner.as_ptr()) }).unwrap(),
            borrowed: false,
            life: self.life.clone(),
        }
    }
}

impl Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Node").field(&self.turtle_token()).finish()
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_node_equals(self.inner.as_ptr(), other.inner.as_ptr()) }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if !self.borrowed {
            let _life = self.life.inner.lock();
            unsafe { lib::lilv_node_free(self.inner.as_ptr()) }
        }
    }
}

/// A collection of `Node`.
pub struct Nodes {
    pub(crate) inner: *const lib::LilvNodes,
    pub(crate) life: Arc<Life>,
}

impl Nodes {
    /// The number of nodes in the collection.
    #[must_use]
    pub fn count(&self) -> usize {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_nodes_size(self.inner) as _ }
    }

    /// Returns `true` if a node with the given value exists in the collection.
    #[must_use]
    pub fn contains(&self, value: &Node) -> bool {
        let _life = self.life.inner.lock();
        let inner = self.inner;
        let value = value.inner.as_ptr();

        unsafe { lib::lilv_nodes_contains(inner, value) }
    }

    /// Return a new collection of `Nodes` that contains all the nodes from `a`
    /// and `b`.
    ///
    /// # Panics
    /// Panics if the merge is unsuccessful.
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        let _life = self.life.inner.lock();
        let a = self.inner;
        let b = other.inner;

        Nodes {
            inner: unsafe { lib::lilv_nodes_merge(a, b) },
            life: self.life.clone(),
        }
    }

    /// An iterator over all the nodes.
    pub fn iter(&self) -> impl '_ + Iterator<Item = Node> {
        let _life = self.life.inner.lock();
        NodesIter {
            inner: unsafe { lib::lilv_nodes_begin(self.inner) },
            life: self.life.clone(),
            nodes: self,
        }
    }
}

impl Debug for Nodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl IntoIterator for Nodes {
    type Item = Node;
    type IntoIter = NodesIter<Nodes>;

    fn into_iter(self) -> NodesIter<Nodes> {
        let inner = unsafe {
            let _life = self.life.inner.lock();
            lib::lilv_nodes_begin(self.inner)
        };
        NodesIter {
            inner,
            life: self.life.clone(),
            nodes: self,
        }
    }
}

/// An iterator through a `Nodes` collection.
pub struct NodesIter<NS> {
    inner: *mut lib::LilvIter,
    life: Arc<Life>,
    nodes: NS,
}

impl<NS> Iterator for NodesIter<NS>
where
    NS: Borrow<Nodes>,
{
    type Item = Node;

    fn next(&mut self) -> Option<Self::Item> {
        let _life = self.life.inner.lock();
        let node = unsafe { lib::lilv_nodes_get(self.nodes.borrow().inner, self.inner) } as *mut _;
        let next = Some({
            let ptr = NonNull::new(node)?;
            let world = self.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        });
        self.inner = unsafe { lib::lilv_nodes_next(self.nodes.borrow().inner, self.inner) };
        next
    }
}

impl<NS> Debug for NodesIter<NS>
where
    NS: Borrow<Nodes>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // tmp can be iterated over without advancing the self iterator.
        let tmp = NodesIter {
            inner: self.inner,
            life: self.life.clone(),
            nodes: self.nodes.borrow(),
        };
        f.debug_list().entries(tmp).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    #[allow(clippy::never_loop)]
    fn test_null_nodes() {
        let world = World::new();
        let nodes = Nodes {
            inner: std::ptr::null(),
            life: world.life,
        };
        assert_eq!(nodes.count(), 0);
        for n in nodes {
            panic!("Should not have any nodes but found {}", n.turtle_token());
        }
    }

    #[test]
    fn test_path() {
        let world = crate::World::with_load_all();
        let uri = world.new_uri("http://lv2plug.in/plugins/eg-amp");
        let plugin = world
            .plugins()
            .plugin(&uri)
            .unwrap_or_else(|| panic!("Could not find plugin {:?}", uri));
        let plugin_library_uri_node = plugin.library_uri().expect("Missing plugin library uri.");
        let (hostname, path) = plugin_library_uri_node
            .path()
            .expect("Missing plugin library path.");
        assert_eq!(hostname, "");
        assert!(path.ends_with("/eg-amp.lv2/amp.so"));
    }
}
