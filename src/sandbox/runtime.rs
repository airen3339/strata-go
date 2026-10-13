// SPDX-License-Identifier: MIT

use std::ffi::{OsStr, OsString};

pub(super) struct Runtime {
    pub(super) path: OsString,
    pub(super) root: OsString,
    pub(super) prlimit: OsString,
    pub(super) gdk_pixbuf_module_file: Option<OsString>,
}

impl Runtime {
    pub(super) fn load() -> Self {
        Self::resolve_with(
            |name| std::env::var_os(name),
            |name| {
                match name {
                    "STRATA_SANDBOX_PATH" => option_env!("STRATA_SANDBOX_PATH"),
                    "STRATA_SANDBOX_ROOT" => option_env!("STRATA_SANDBOX_ROOT"),
                    "STRATA_SANDBOX_PRLIMIT" => option_env!("STRATA_SANDBOX_PRLIMIT"),
                    "STRATA_SANDBOX_GDK_PIXBUF_MODULE_FILE" => {
                        option_env!("STRATA_SANDBOX_GDK_PIXBUF_MODULE_FILE")
                    }
                    _ => None,
                }
                .map(OsString::from)
            },
        )
    }

    fn resolve_with(
        mut read_runtime: impl FnMut(&str) -> Option<OsString>,
        mut read_embedded: impl FnMut(&str) -> Option<OsString>,
    ) -> Self {
        Self {
            path: read_runtime("STRATA_SANDBOX_PATH")
                .or_else(|| read_embedded("STRATA_SANDBOX_PATH"))
                .unwrap_or_else(|| OsString::from("/usr/bin")),
            root: read_runtime("STRATA_SANDBOX_ROOT")
                .filter(|root| runtime_root_allowed(root))
                .or_else(|| read_embedded("STRATA_SANDBOX_ROOT"))
                .unwrap_or_else(|| OsString::from("/usr")),
            prlimit: read_runtime("STRATA_SANDBOX_PRLIMIT")
                .or_else(|| read_embedded("STRATA_SANDBOX_PRLIMIT"))
                .unwrap_or_else(|| OsString::from("/usr/bin/prlimit")),
            gdk_pixbuf_module_file: read_runtime("STRATA_SANDBOX_GDK_PIXBUF_MODULE_FILE")
                .or_else(|| read_embedded("STRATA_SANDBOX_GDK_PIXBUF_MODULE_FILE")),
        }
    }
}

fn runtime_root_allowed(root: &OsStr) -> bool {
    // Exact spellings prevent traversal and accidental exposure of user-data trees.
    ["/usr", "/nix/store", "/gnu/store"]
        .iter()
        .any(|allowed| root == OsStr::new(allowed))
}

#[cfg(test)]
mod tests;
