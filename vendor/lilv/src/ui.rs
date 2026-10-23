use crate::node::{Node, Nodes};
use crate::plugin::Plugin;
use crate::world::Life;
use lilv_sys as lib;
use std::ffi::CStr;
use std::ptr::NonNull;
use std::sync::Arc;

pub struct UI {
    pub(crate) inner: NonNull<lib::LilvUI>,
    pub(crate) plugin: Plugin,
    pub(crate) life: Arc<Life>,
}

impl UI {
    /// Get the URI of a Plugin UI.
    ///
    /// # Panics
    /// Panics if it was not possible to get the URI.
    #[must_use]
    pub fn uri(&self) -> Node {
        let _life = self.life.inner.lock();
        let ui = self.inner.as_ptr();

        {
            let ptr = NonNull::new(unsafe { lib::lilv_ui_get_uri(ui) as _ }).unwrap();
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        }
    }

    /// Get the types (URIs of RDF classes) of a Plugin UI.
    #[must_use]
    pub fn classes(&self) -> Nodes {
        let _life = self.life.inner.lock();
        let ui = self.inner.as_ptr();

        let inner = unsafe { lib::lilv_ui_get_classes(ui) };
        let world = self.plugin.life.clone();
        Nodes { inner, life: world }
    }

    /// Returns `true` if the plugin UI has a given type.
    #[must_use]
    pub fn is_a(&self, class_uri: &Node) -> bool {
        let _life = self.life.inner.lock();
        let ui = self.inner.as_ptr();
        let class_uri = class_uri.inner.as_ptr();

        unsafe { lib::lilv_ui_is_a(ui, class_uri) }
    }

    #[must_use]
    pub fn is_supported<S>(
        &self,
        container_type: &Node,
        ui_type: Option<&mut Option<Node>>,
    ) -> UISupportQuality
    where
        S: UISupport,
    {
        let ui = self.inner.as_ptr();
        let container_type = container_type.inner.as_ptr();

        let mut ui_type_ptr = std::ptr::null();

        let quality = UISupportQuality(unsafe {
            let _life = self.life.inner.lock();
            lib::lilv_ui_is_supported(
                ui,
                Some(supported_func::<S>),
                container_type,
                ui_type
                    .as_ref()
                    .map_or(std::ptr::null_mut(), |_| &mut ui_type_ptr as _),
            )
        });

        if let Some(ui_type) = ui_type {
            let ptr = match NonNull::new(ui_type_ptr as _) {
                Some(ptr) => ptr,
                None => return UISupportQuality(0),
            };
            *ui_type = Some({
                let world = self.plugin.life.clone();
                Node {
                    inner: ptr,
                    borrowed: true,
                    life: world,
                }
            });
        }

        quality
    }

    #[must_use]
    pub fn bundle_uri(&self) -> Option<Node> {
        let ui = self.inner.as_ptr();
        let _life = self.life.inner.lock();

        Some({
            let ptr = NonNull::new(unsafe { lib::lilv_ui_get_bundle_uri(ui) as _ })?;
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        })
    }

    /// Get the uri for the binary.
    #[must_use]
    pub fn binary_uri(&self) -> Option<Node> {
        let _life = self.life.inner.lock();
        let ui = self.inner.as_ptr();

        Some({
            let ptr = NonNull::new(unsafe { lib::lilv_ui_get_binary_uri(ui) as _ })?;
            let world = self.plugin.life.clone();
            Node {
                inner: ptr,
                borrowed: true,
                life: world,
            }
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UISupportQuality(pub u32);

pub trait UISupport {
    fn supported(container: &str, ui: &str) -> UISupportQuality;
}

unsafe extern "C" fn supported_func<S: UISupport>(
    container_type_uri: *const i8,
    ui_type_uri: *const i8,
) -> u32 {
    S::supported(
        CStr::from_ptr(container_type_uri).to_str().unwrap(),
        CStr::from_ptr(ui_type_uri).to_str().unwrap(),
    )
    .0
}

/// A collection of plugin UIs.
#[derive(Clone)]
pub struct Uis {
    pub(crate) inner: NonNull<lib::LilvUIs>,
    pub(crate) plugin: Plugin,
    pub(crate) life: Arc<Life>,
}

impl Uis {
    #[must_use]
    pub fn count(&self) -> usize {
        let _life = self.life.inner.lock();
        unsafe { lib::lilv_uis_size(self.inner.as_ptr()) as _ }
    }

    #[must_use]
    pub fn get_by_uri(&self, uri: &Node) -> Option<UI> {
        let _life = self.life.inner.lock();
        let inner = self.inner.as_ptr();
        let uri = uri.inner.as_ptr();

        Some(UI {
            inner: NonNull::new(unsafe { lib::lilv_plugins_get_by_uri(inner, uri) as _ })?,
            plugin: self.plugin.clone(),
            life: self.life.clone(),
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = UI> {
        self.clone().into_iter()
    }
}

impl IntoIterator for Uis {
    type Item = UI;

    type IntoIter = Iter;

    fn into_iter(self) -> Self::IntoIter {
        let iter = unsafe {
            let _life = self.life.inner.lock();
            lib::lilv_uis_begin(self.inner.as_ptr()).cast()
        };
        Iter { uis: self, iter }
    }
}

/// An iterator over the UIs of a plugin.
pub struct Iter {
    uis: Uis,
    iter: *mut lib::LilvIter,
}

impl Iterator for Iter {
    type Item = UI;

    fn next(&mut self) -> Option<UI> {
        let _life = self.uis.life.inner.lock();
        let next =
            unsafe { lib::lilv_uis_get(self.uis.inner.as_ptr(), self.iter) } as *mut lib::LilvUI;
        let ret = Some(UI {
            inner: NonNull::new(next)?,
            plugin: self.uis.plugin.clone(),
            life: self.uis.life.clone(),
        });
        self.iter = unsafe { lib::lilv_uis_next(self.uis.inner.as_ptr(), self.iter) };
        ret
    }
}
